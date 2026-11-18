#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod net;

use serde::Serialize;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

const PING_TIMEOUT_MS: u32 = 1000;
const ROUND_INTERVAL: Duration = Duration::from_millis(1500);
/// この回数連続で応答がなければ「いない」扱いにする (ちらつき防止)
const MISS_LIMIT: u32 = 2;
/// 名前が取れなかったホストを再問い合わせするまでの間隔
const NAME_RETRY: Duration = Duration::from_secs(60);

/// 選択中の送信元アドレス。変更のたびに世代番号を進め、古いスキャン結果を捨てる。
struct Scanner {
    gen: AtomicU64,
    src: Mutex<Option<Ipv4Addr>>,
}

enum NameState {
    Pending,
    Done(Option<String>, Instant),
}

type Names = Arc<Mutex<(u64, HashMap<u8, NameState>)>>;

#[derive(Serialize, Clone)]
struct Host {
    alive: bool,
    name: Option<String>,
}

#[derive(Serialize, Clone)]
struct ScanPayload {
    src: String,
    hosts: Vec<Host>,
}

#[tauri::command]
fn get_adapters() -> Vec<net::Adapter> {
    net::list_adapters()
}

#[tauri::command]
fn set_source(ip: String, scanner: State<'_, Arc<Scanner>>) -> Result<(), String> {
    let ip: Ipv4Addr = ip.parse().map_err(|_| format!("不正な IP アドレス: {ip}"))?;
    *scanner.src.lock().unwrap() = Some(ip);
    scanner.gen.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

fn scan_loop(app: AppHandle, scanner: Arc<Scanner>) {
    let names: Names = Arc::new(Mutex::new((0, HashMap::new())));
    let mut cur_gen = u64::MAX;
    let mut miss = [u32::MAX; 256];

    loop {
        let started = Instant::now();
        let gen = scanner.gen.load(Ordering::SeqCst);
        let Some(src) = *scanner.src.lock().unwrap() else {
            thread::sleep(Duration::from_millis(200));
            continue;
        };
        if gen != cur_gen {
            cur_gen = gen;
            miss = [u32::MAX; 256];
            *names.lock().unwrap() = (gen, HashMap::new());
        }

        let [a, b, c, _] = src.octets();
        let results: Vec<(u8, bool)> = thread::scope(|s| {
            let handles: Vec<_> = (1..=254u8)
                .map(|i| {
                    s.spawn(move || (i, net::ping(src, Ipv4Addr::new(a, b, c, i), PING_TIMEOUT_MS)))
                })
                .collect();
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        });

        // スキャン中にアダプタが切り替わったら結果は捨てる
        if scanner.gen.load(Ordering::SeqCst) != gen {
            continue;
        }

        for (i, ok) in &results {
            let m = &mut miss[*i as usize];
            *m = if *ok { 0 } else { m.saturating_add(1) };
        }

        let mut hosts = Vec::with_capacity(256);
        {
            let mut guard = names.lock().unwrap();
            for i in 0..=255u8 {
                let alive = miss[i as usize] < MISS_LIMIT;
                let mut name = None;
                if alive {
                    let need_lookup = match guard.1.get(&i) {
                        None => true,
                        Some(NameState::Done(None, at)) => at.elapsed() > NAME_RETRY,
                        _ => false,
                    };
                    if need_lookup {
                        guard.1.insert(i, NameState::Pending);
                        let names = names.clone();
                        let ip = Ipv4Addr::new(a, b, c, i);
                        thread::spawn(move || {
                            let n = net::resolve_name(ip);
                            let mut g = names.lock().unwrap();
                            if g.0 == gen {
                                g.1.insert(i, NameState::Done(n, Instant::now()));
                            }
                        });
                    }
                    if let Some(NameState::Done(Some(n), _)) = guard.1.get(&i) {
                        name = Some(n.clone());
                    }
                }
                hosts.push(Host { alive, name });
            }
        }

        let _ = app.emit("scan", ScanPayload { src: src.to_string(), hosts });

        let elapsed = started.elapsed();
        if elapsed < ROUND_INTERVAL {
            thread::sleep(ROUND_INTERVAL - elapsed);
        }
    }
}

fn main() {
    let scanner = Arc::new(Scanner { gen: AtomicU64::new(0), src: Mutex::new(None) });

    tauri::Builder::default()
        .manage(scanner.clone())
        .invoke_handler(tauri::generate_handler![get_adapters, set_source])
        .setup(move |app| {
            let handle = app.handle().clone();
            let scanner = app.state::<Arc<Scanner>>().inner().clone();
            thread::spawn(move || scan_loop(handle, scanner));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("ViewIP の起動に失敗しました");
}
