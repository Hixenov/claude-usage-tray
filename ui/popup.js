// Popup page. All text arrives from Rust already localized; this file only renders it.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);

function applyStrings(strings) {
  for (const el of document.querySelectorAll("[data-t]")) {
    const text = strings[el.dataset.t];
    if (text) el.textContent = text;
  }
}

function quotaItem(q) {
  const li = document.createElement("li");
  li.className = "quota";

  const row = document.createElement("div");
  row.className = "row";
  const name = document.createElement("span");
  name.textContent = q.name;
  const percent = document.createElement("span");
  percent.className = "percent";
  percent.textContent = `${q.percent}%`;
  row.append(name, percent);

  const bar = document.createElement("div");
  bar.className = "bar";
  const fill = document.createElement("div");
  fill.className = `fill ${q.color}`;
  fill.style.width = `${q.percent}%`;
  bar.append(fill);
  if (q.elapsed !== null) {
    const marker = document.createElement("div");
    marker.className = "marker";
    marker.style.left = `${(q.elapsed * 100).toFixed(2)}%`;
    bar.append(marker);
  }

  li.append(row, bar);
  if (q.reset_text) {
    const reset = document.createElement("div");
    reset.className = "reset";
    reset.textContent = q.reset_text;
    li.append(reset);
  }
  return li;
}

function render(view) {
  $("account").textContent = [view.email, view.plan].filter(Boolean).join(" · ");
  $("error").hidden = !view.error_text;
  $("error").textContent = view.error_text || "";
  $("stale").hidden = !view.stale;
  $("quotas").replaceChildren(...view.quotas.map(quotaItem));
  $("loading").hidden = view.quotas.length > 0 || Boolean(view.error_text);
  $("updated").textContent = view.updated_text || "";
  fit();
}

function fit() {
  const height = Math.ceil(document.querySelector("main").getBoundingClientRect().height);
  invoke("fit_popup", { height });
}

async function init() {
  applyStrings(await invoke("get_strings"));
  render(await invoke("get_state"));
  await listen("state", (event) => render(event.payload));
  $("refresh").addEventListener("click", () => invoke("refresh"));
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") invoke("close_popup");
  });
}

init();
