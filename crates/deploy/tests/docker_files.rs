//! Tests for Dockerfile and compose.yaml web deployment configuration.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

fn docker_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docker")
}

#[test]
fn dockerfile_defines_web_bundle_and_scratch_web_stage() {
    let text = fs::read_to_string(docker_dir().join("Dockerfile")).unwrap();
    let stages: Vec<&str> = text.split("\nFROM ").collect();

    let bundle_stage = stages
        .iter()
        .find(|s| s.starts_with("build AS web-bundle"))
        .expect("missing web-bundle stage FROM build");
    assert!(bundle_stage.contains("wasm32-unknown-unknown"));
    assert!(bundle_stage.contains("riscv64gc-unknown-none-elf"));
    assert!(bundle_stage.contains("cargo run --release --locked -p site --example pack"));

    let web_stage = stages
        .iter()
        .find(|s| s.starts_with("scratch AS web"))
        .expect("missing web stage FROM scratch");
    assert!(web_stage.contains("/monolith-web"));
    assert!(web_stage.contains("/healthcheck"));
    assert!(web_stage.contains("/www"));
    assert!(web_stage.contains("USER 65534"));
    assert!(web_stage.contains("EXPOSE 8080"));
    assert!(web_stage.contains("HEALTHCHECK "));
    assert!(web_stage.contains("127.0.0.1:8080"));
    assert!(web_stage.contains("/healthz"));
}

#[test]
fn compose_exposes_only_web_publicly_and_keeps_others_loopback() {
    let text = fs::read_to_string(docker_dir().join("compose.yaml")).unwrap();

    let mut in_services = false;
    let mut current_service: Option<String> = None;
    let mut in_ports = false;
    let mut service_ports: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut web_lines = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if line == "services:" {
            in_services = true;
            continue;
        }
        if !line.starts_with(' ') && line.ends_with(':') {
            in_services = false;
            current_service = None;
            in_ports = false;
            continue;
        }
        if !in_services {
            continue;
        }

        let indent = line.bytes().take_while(|&b| b == b' ').count();
        if indent == 2 && trimmed.ends_with(':') {
            let name = trimmed.trim_end_matches(':').to_string();
            service_ports.entry(name.clone()).or_default();
            current_service = Some(name);
            in_ports = false;
            continue;
        }

        if current_service.as_deref() == Some("web") {
            web_lines.push(line);
        }

        if indent == 4 && trimmed == "ports:" {
            in_ports = true;
            continue;
        }
        if indent <= 4 {
            in_ports = false;
        }
        if in_ports && trimmed.starts_with('-') {
            let port = trimmed
                .trim_start_matches('-')
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            if let Some(svc) = &current_service {
                service_ports.entry(svc.clone()).or_default().push(port);
            }
        }
    }

    let web_ports = service_ports.get("web").expect("missing web service");
    assert_eq!(web_ports, &vec!["8080:8080".to_string()]);
    let web_block = web_lines.join("\n");
    assert!(web_block.contains("read_only: true"));
    assert!(web_block.contains("healthcheck:"));
    assert!(web_block.contains("/healthz"));

    assert!(service_ports.len() >= 2, "expected private services too");
    for (svc, ports) in &service_ports {
        if svc == "web" {
            continue;
        }
        assert!(!ports.is_empty(), "expected loopback port entry for {svc}");
        for p in ports {
            assert!(
                p.starts_with("127.0.0.1:"),
                "non-web service {svc} exposes non-loopback port {p}"
            );
        }
    }
}
