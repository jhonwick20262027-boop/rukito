//! Wrapper de rustc para este workspace (x86_64-pc-windows-gnu).
//!
//! Cargo lo invoca como `rustc-gnu <ruta-a-rustc> <args...>`. Antepone
//! `C:\mingw64\bin` al PATH del proceso rustc y de sus hijos (gcc, ld,
//! dlltool, as). Sin esto, en esta máquina:
//!
//! * rustc no encuentra `dlltool` para las librerías raw-dylib (windows-sys);
//! * `dlltool` no encuentra `as.exe` y genera un `.lib` de 0 bytes que rustc
//!   rechaza con «The volume for a file has been externally altered».
//!
//! Es un ejecutable y no un `.bat` porque la línea de comandos de `windows-sys`
//! supera el límite de 8191 caracteres de `cmd.exe`.
//!
//! Se recompila con:
//!
//! ```text
//! rustc -O scripts/rustc-gnu.rs -o scripts/rustc-gnu.exe
//! ```
//!
//! (con `C:\mingw64\bin` en el PATH de esa compilación puntual).

use std::env;
use std::ffi::OsString;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let mut args = env::args_os();
    let _wrapper = args.next();
    let Some(rustc) = args.next() else {
        eprintln!("rustc-gnu: cargo debe pasar la ruta de rustc como primer argumento");
        return ExitCode::FAILURE;
    };

    let mut path = OsString::from(r"C:\mingw64\bin;");
    path.push(env::var_os("PATH").unwrap_or_default());

    match Command::new(rustc).args(args).env("PATH", path).status() {
        Ok(status) => ExitCode::from(status.code().unwrap_or(1) as u8),
        Err(err) => {
            eprintln!("rustc-gnu: no se pudo ejecutar rustc: {err}");
            ExitCode::FAILURE
        }
    }
}
