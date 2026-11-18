//! Windows ネットワーク処理: アダプタ一覧取得と、送信元アドレスを固定した ping。

use serde::Serialize;
use std::net::{IpAddr, Ipv4Addr};
use windows_sys::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, INVALID_HANDLE_VALUE, NO_ERROR};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho2Ex, GAA_FLAG_SKIP_ANYCAST,
    GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST, ICMP_ECHO_REPLY, IP_ADAPTER_ADDRESSES_LH,
};

const AF_INET: u32 = 2;
const IF_TYPE_ETHERNET: u32 = 6;
const IF_TYPE_LOOPBACK: u32 = 24;
const IF_TYPE_WIFI: u32 = 71;
const IF_OPER_UP: i32 = 1;

#[derive(Serialize, Clone, Debug)]
pub struct Adapter {
    /// 表示名 (例: "イーサネット", "Wi-Fi")
    pub name: String,
    pub description: String,
    pub ip: String,
    pub prefix: u8,
    /// "wired" | "wifi" | "other"
    pub kind: String,
}

unsafe fn wstr(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0;
    while *p.add(len) != 0 {
        len += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
}

/// 稼働中で IPv4 アドレスを持つアダプタの一覧 (アドレスごとに 1 エントリ)。
pub fn list_adapters() -> Vec<Adapter> {
    let mut size: u32 = 16 * 1024;
    let mut buf: Vec<u64>;
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    loop {
        buf = vec![0u64; (size as usize + 7) / 8];
        let r = unsafe {
            GetAdaptersAddresses(
                AF_INET,
                flags,
                std::ptr::null(),
                buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH,
                &mut size,
            )
        };
        if r == ERROR_BUFFER_OVERFLOW {
            continue;
        }
        if r != NO_ERROR {
            return Vec::new();
        }
        break;
    }

    let mut out = Vec::new();
    let mut p = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while !p.is_null() {
        let a = unsafe { &*p };
        p = a.Next;
        if a.IfType == IF_TYPE_LOOPBACK || a.OperStatus as i32 != IF_OPER_UP {
            continue;
        }
        let kind = match a.IfType {
            IF_TYPE_ETHERNET => "wired",
            IF_TYPE_WIFI => "wifi",
            _ => "other",
        };
        let name = unsafe { wstr(a.FriendlyName) };
        let description = unsafe { wstr(a.Description) };
        let mut u = a.FirstUnicastAddress;
        while !u.is_null() {
            let ua = unsafe { &*u };
            u = ua.Next;
            let sa = ua.Address.lpSockaddr as *const u8;
            if sa.is_null() {
                continue;
            }
            // SOCKADDR_IN: family(u16) port(u16) addr(4 bytes)
            let family = unsafe { std::ptr::read_unaligned(sa as *const u16) };
            if family as u32 != AF_INET {
                continue;
            }
            let b = unsafe { std::slice::from_raw_parts(sa.add(4), 4) };
            let ip = Ipv4Addr::new(b[0], b[1], b[2], b[3]);
            out.push(Adapter {
                name: name.clone(),
                description: description.clone(),
                ip: ip.to_string(),
                prefix: ua.OnLinkPrefixLength,
                kind: kind.to_string(),
            });
        }
    }
    out
}

/// `src` を送信元アドレスにして `dst` へ ICMP Echo を 1 回送る。
/// 送信元を固定するので、そのアドレスを持つアダプタから送信される。管理者権限は不要。
pub fn ping(src: Ipv4Addr, dst: Ipv4Addr, timeout_ms: u32) -> bool {
    let data = [0x61u8; 32];
    let mut reply =
        vec![0u64; (std::mem::size_of::<ICMP_ECHO_REPLY>() + data.len() + 8 + 64 + 7) / 8];
    let dst_raw = u32::from_ne_bytes(dst.octets());
    unsafe {
        let h = IcmpCreateFile();
        if h == INVALID_HANDLE_VALUE {
            return false;
        }
        let n = IcmpSendEcho2Ex(
            h,
            std::ptr::null_mut(),
            None,
            std::ptr::null(),
            u32::from_ne_bytes(src.octets()),
            dst_raw,
            data.as_ptr() as *const _,
            data.len() as u16,
            std::ptr::null(),
            reply.as_mut_ptr() as *mut _,
            (reply.len() * 8) as u32,
            timeout_ms,
        );
        IcmpCloseHandle(h);
        if n == 0 {
            return false;
        }
        let r = &*(reply.as_ptr() as *const ICMP_ECHO_REPLY);
        r.Status == 0 && r.Address == dst_raw
    }
}

/// 逆引きでホスト名を取得 (DNS / NetBIOS / LLMNR)。取れなければ None。
pub fn resolve_name(ip: Ipv4Addr) -> Option<String> {
    let name = dns_lookup::lookup_addr(&IpAddr::V4(ip)).ok()?;
    let name = name.trim_end_matches('.');
    if name.is_empty() || name.parse::<IpAddr>().is_ok() {
        return None;
    }
    // igping 同様、短い名前 (最初のラベル) だけを表示する
    Some(name.split('.').next().unwrap_or(name).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 実ネットワークを使う確認用: cargo test -- --ignored --nocapture
    #[test]
    #[ignore]
    fn scan_each_adapter() {
        for a in list_adapters() {
            let src: Ipv4Addr = a.ip.parse().unwrap();
            let [x, y, z, _] = src.octets();
            let alive: Vec<u8> = std::thread::scope(|s| {
                let hs: Vec<_> = (1..=254u8)
                    .map(|i| s.spawn(move || (i, ping(src, Ipv4Addr::new(x, y, z, i), 1000))))
                    .collect();
                hs.into_iter().map(|h| h.join().unwrap()).filter(|r| r.1).map(|r| r.0).collect()
            });
            println!("{} [{}] {}/{} -> {} hosts: {:?}", a.name, a.kind, a.ip, a.prefix, alive.len(), alive);
            if let Some(&i) = alive.first() {
                println!("  name of .{}: {:?}", i, resolve_name(Ipv4Addr::new(x, y, z, i)));
            }
        }
    }
}
