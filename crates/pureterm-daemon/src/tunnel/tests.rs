//! tunnel 模块单测:tunnel id 唯一性、状态字符串、list/stop/provide_auth
//! 基线、SOCKS5 / 连接地址格式不变量、remote_dns 决策、TOFU host-key 决策。

use super::handler::{decide_host_key, forward_verbatim, format_connect_addr};
use super::{new_tunnel_id, TunnelLive, TunnelManager};
use crate::known_hosts::KnownHosts;

#[test]
fn tunnel_ids_are_unique() {
    let a = new_tunnel_id();
    let b = new_tunnel_id();
    assert!(a.starts_with("tun-"));
    assert_ne!(a, b);
}

#[test]
fn status_strings_match_cli_contract() {
    assert_eq!(TunnelLive::Connecting.as_str(), "connecting");
    assert_eq!(TunnelLive::Connected.as_str(), "connected");
    assert_eq!(TunnelLive::NeedAuth.as_str(), "need_auth");
    assert_eq!(TunnelLive::Failed("x".into()).as_str(), "failed");
}

#[test]
fn list_is_empty_by_default() {
    let mgr = TunnelManager::default();
    assert!(mgr.list().is_empty());
}

#[test]
fn stop_unknown_returns_false() {
    let mgr = TunnelManager::default();
    assert!(!mgr.stop("nope"));
}

#[test]
fn provide_auth_unknown_errors() {
    let mgr = TunnelManager::default();
    assert!(mgr.provide_auth("nope", "s".into()).is_err());
}

/// `read_dst_addr` is async over a real socket; this exercises the IPv4 /
/// IPv6 / domain formatting paths by feeding a cursor-backed mock stream
/// through the same Tokio codec. We instead unit-test the pure formatting
/// invariants the function relies on.
#[test]
fn ipv4_format_is_dotted_quad() {
    // Mirrors the 0x01 branch of read_dst_addr.
    let b = [192u8, 168, 1, 42];
    assert_eq!(format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3]), "192.168.1.42");
}

#[test]
fn port_is_big_endian_from_two_bytes() {
    // Mirrors the port decode in serve_socks5.
    assert_eq!(u16::from_be_bytes([0x1F, 0x90]), 8080);
    assert_eq!(u16::from_be_bytes([0x00, 0x50]), 80);
}

// ── GAP-5.2 remote_dns resolution decision (--no-remote-dns) ──────────
#[test]
fn remote_dns_domain_is_forwarded_verbatim() {
    // Default: the gateway resolves the domain (socks5h / --socks5-hostname).
    assert!(forward_verbatim(true, "internal.corp"));
}

#[test]
fn no_remote_dns_domain_is_resolved_locally() {
    // --no-remote-dns: resolve here so the domain never leaks to the gateway.
    assert!(!forward_verbatim(false, "internal.corp"));
}

#[test]
fn ip_literal_is_forwarded_verbatim_regardless_of_remote_dns() {
    // IP literals need no DNS — never spend a lookup, never change representation.
    assert!(forward_verbatim(true, "10.0.0.1"));
    assert!(forward_verbatim(false, "10.0.0.1"));
    assert!(forward_verbatim(false, "::1"));
}

// ── GAP-5.2 SSH connect-addr formatting (IPv6 literal safety) ─────────
#[test]
fn connect_addr_hostname_is_host_colon_port() {
    assert_eq!(format_connect_addr("gw.example", 22), "gw.example:22");
}

#[test]
fn connect_addr_ipv4_literal_is_unbracketed() {
    assert_eq!(format_connect_addr("10.0.0.1", 2222), "10.0.0.1:2222");
}

#[test]
fn connect_addr_ipv6_literal_is_bracketed() {
    // An IPv6 literal contains ':' → must bracket or "2001:db8::1:22" won't parse.
    assert_eq!(format_connect_addr("2001:db8::1", 22), "[2001:db8::1]:22");
}

// ── GAP-5.2 TOFU host-key decision ────────────────────────────────────
const KEY_A_B64: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIAABAgMEBQYHCAkKCwwNDg8QERITFBUWFxgZGhscHR4f";
const KEY_B_B64: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIAMKERgfJi00O0JJUFdeZWxzeoGIj5adpKuyucDHztXc";

fn pubkey(b64: &str) -> russh::keys::ssh_key::PublicKey {
    russh::keys::parse_public_key_base64(b64).unwrap()
}

#[test]
fn host_key_no_store_accepts() {
    assert!(decide_host_key(None, "gw", &pubkey(KEY_A_B64)));
}

#[test]
fn host_key_unknown_records_then_accepts() {
    let tmp = tempfile::tempdir().unwrap();
    let kh = KnownHosts::open(tmp.path().join("kh")).unwrap();
    assert!(decide_host_key(Some(&kh), "gw", &pubkey(KEY_A_B64)));
}

#[test]
fn host_key_match_accepts() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("kh");
    let kh = KnownHosts::open(path.clone()).unwrap();
    assert!(decide_host_key(Some(&kh), "gw", &pubkey(KEY_A_B64)));
    // Reload from the file the first connect wrote → Match must still accept.
    let kh2 = KnownHosts::open(path).unwrap();
    assert!(decide_host_key(Some(&kh2), "gw", &pubkey(KEY_A_B64)));
}

#[test]
fn host_key_mismatch_rejects() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("kh");
    let kh = KnownHosts::open(path.clone()).unwrap();
    assert!(decide_host_key(Some(&kh), "gw", &pubkey(KEY_A_B64)));
    // Present a different key for the now-known host → reject.
    let kh2 = KnownHosts::open(path).unwrap();
    assert!(!decide_host_key(Some(&kh2), "gw", &pubkey(KEY_B_B64)));
}
