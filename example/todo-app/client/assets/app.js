// El script viaja dentro del prefijo estático y hereda la caché inmutable de
// `/site/assets`; por eso su URL no cambia al editar el HTML. Para producción,
// renombra el archivo o añade una versión al `src` (RF-258).
const health = document.querySelector("#health");

fetch("/health", { headers: { accept: "application/json" } })
  .then((resp) => {
    if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
    return resp.json();
  })
  .then((body) => {
    health.textContent = `salud ok · ${body.status ?? "ok"}`;
    health.classList.add("ok");
  })
  .catch((err) => {
    health.textContent = `salud no disponible · ${err.message}`;
    health.classList.add("bad");
  });
