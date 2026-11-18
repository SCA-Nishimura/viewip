const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const grid = document.getElementById("grid");
const select = document.getElementById("adapter");
const info = document.getElementById("info");

const KIND = { wired: "有線", wifi: "Wi-Fi", other: "その他" };
const STORE_KEY = "viewip.adapter";

let adapters = [];
let currentIp = null;

// 0〜255 のセルを作る (4列 × 64行、列方向に並ぶ)
const cells = [];
for (let i = 0; i < 256; i++) {
  const el = document.createElement("div");
  el.className = "cell";
  el.innerHTML = `<span class="n">${i}</span><span class="h"></span>`;
  grid.appendChild(el);
  cells.push({ el, name: el.querySelector(".h"), alive: false });
}

function clearCells() {
  for (const c of cells) {
    c.el.classList.remove("alive");
    c.name.textContent = "";
    c.alive = false;
  }
}

const key = (a) => `${a.name}|${a.ip}`;

async function loadAdapters() {
  adapters = await invoke("get_adapters");
  const prev = select.value || localStorage.getItem(STORE_KEY);

  select.innerHTML = "";
  for (const a of adapters) {
    const opt = document.createElement("option");
    opt.value = key(a);
    opt.textContent = `[${KIND[a.kind]}] ${a.name}  ${a.ip}/${a.prefix}`;
    opt.title = a.description;
    select.appendChild(opt);
  }

  if (adapters.length === 0) {
    info.textContent = "使用できるアダプタがありません";
    return;
  }

  // 前回の選択 → 有線 → 先頭 の順で選ぶ
  const pick =
    adapters.find((a) => key(a) === prev) ||
    adapters.find((a) => a.kind === "wired") ||
    adapters[0];
  select.value = key(pick);
  if (pick.ip !== currentIp) await apply(pick);
}

async function apply(a) {
  currentIp = a.ip;
  clearCells();
  const [x, y, z] = a.ip.split(".");
  info.textContent = `${x}.${y}.${z}.0 - 255 をスキャン中`;
  localStorage.setItem(STORE_KEY, key(a));
  await invoke("set_source", { ip: a.ip });
}

select.addEventListener("change", () => {
  const a = adapters.find((a) => key(a) === select.value);
  if (a) apply(a);
});

// ウィンドウに戻ってきたらアダプタ一覧を更新 (LAN ケーブルの抜き差しなど)
window.addEventListener("focus", () => loadAdapters());

listen("scan", ({ payload }) => {
  if (payload.src !== currentIp) return;
  payload.hosts.forEach((h, i) => {
    const c = cells[i];
    if (c.alive !== h.alive) {
      c.el.classList.toggle("alive", h.alive);
      c.alive = h.alive;
    }
    const name = h.alive && h.name ? h.name : "";
    if (c.name.textContent !== name) c.name.textContent = name;
  });
});

loadAdapters();
