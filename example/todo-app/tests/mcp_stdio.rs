//! M-09 (§20, §26.7): el demo arranca con `--mcp`, crea, lista y completa una
//! tarea por stdio, sin abrir puerto, y termina con código 0 al cerrar `stdin`.

use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::timeout;
use uuid::Uuid;

const WAIT: Duration = Duration::from_secs(20);
/// El demo exige `JWT_SECRET` para montar el middleware, aunque el modo MCP no
/// valide tokens: es la misma configuración de §26.3.
const SECRET: &str = "secreto-de-prueba-lo-bastante-largo-para-hs256";

struct Demo {
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    child: Child,
}

impl Demo {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_todo-app"))
            .arg("--mcp")
            .env("RDD_MEMORY", "1")
            .env("JWT_SECRET", SECRET)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .expect("no arrancó todo-app --mcp");
        let stdin = child.stdin.take().expect("sin stdin");
        let stdout = child.stdout.take().expect("sin stdout");
        Self {
            stdin,
            lines: BufReader::new(stdout).lines(),
            child,
        }
    }

    async fn send(&mut self, value: Value) {
        let mut line = value.to_string();
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await.unwrap();
        self.stdin.flush().await.unwrap();
    }

    async fn recv(&mut self) -> Value {
        let line = timeout(WAIT, self.lines.next_line())
            .await
            .expect("el demo no respondió a tiempo")
            .unwrap()
            .expect("el demo cerró stdout");
        serde_json::from_str(&line).unwrap_or_else(|_| panic!("línea no JSON en stdout: {line:?}"))
    }

    async fn call(&mut self, id: u64, name: &str, arguments: Value) -> Value {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        }))
        .await;
        let response = self.recv().await;
        assert_eq!(response["id"], id);
        assert!(
            response.get("error").is_none(),
            "tools/call `{name}` falló: {response}"
        );
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("sin contenido de texto: {response}"));
        parse(text)
    }

    async fn finish(mut self) {
        drop(self.stdin);
        match timeout(Duration::from_millis(500), self.lines.next_line()).await {
            Ok(Ok(Some(line))) => panic!("stdout llevó una línea inesperada: {line}"),
            Ok(Err(err)) => panic!("fallo leyendo stdout: {err}"),
            Ok(Ok(None)) | Err(_) => {}
        }
        let status = timeout(WAIT, self.child.wait())
            .await
            .expect("el demo no terminó con EOF")
            .unwrap();
        assert!(status.success(), "el demo salió con {status}");
    }
}

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|_| panic!("contenido no JSON: {text:?}"))
}

#[tokio::test]
async fn m09_el_demo_crea_lista_y_completa_por_stdio() {
    let mut demo = Demo::start();

    demo.send(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "m09", "version": "1" }
        }
    }))
    .await;
    let init = demo.recv().await;
    assert_eq!(init["id"], 1);
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    demo.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
        .await;

    demo.send(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }))
        .await;
    let list = demo.recv().await;
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["create_task", "list_tasks", "complete_task"]);

    let created = demo
        .call(3, "create_task", json!({ "title": "comprar pan" }))
        .await;
    assert_eq!(created["title"], "comprar pan");
    assert_eq!(created["done"], false);
    assert_eq!(created["owner_id"], Uuid::nil().to_string());
    let id = created["id"].as_str().expect("id de la tarea").to_owned();

    let listed = demo.call(4, "list_tasks", json!({})).await;
    let listed = listed.as_array().expect("arreglo de tareas");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["id"], id);

    let completed = demo.call(5, "complete_task", json!({ "id": id })).await;
    assert_eq!(completed["id"], id);
    assert_eq!(completed["done"], true);

    let done = demo.call(6, "list_tasks", json!({ "done": true })).await;
    assert_eq!(done.as_array().unwrap().len(), 1);

    demo.finish().await;
}
