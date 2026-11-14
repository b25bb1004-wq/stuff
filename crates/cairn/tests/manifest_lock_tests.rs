//! Manifest and lock file tests for cairn.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;

use assay::{forall, Rng};
use cairn::lock;
use cairn::manifest::Manifest;
use cairn::solve::Solution;
use cairn::version::{Req, Version};

fn gen_name(rng: &mut Rng) -> String {
    let len = rng.range(1, 10) as usize;
    let mut s = String::with_capacity(len);
    let first = (b'a' + (rng.below(26) as u8)) as char;
    s.push(first);
    let alphabet = b"abcdefghijklmnopqrstuvwxyz0123456789_-";
    for _ in 1..len {
        let ch = alphabet[rng.below(alphabet.len())] as char;
        s.push(ch);
    }
    s
}

fn gen_version(rng: &mut Rng) -> Version {
    Version {
        major: rng.range(0, 10),
        minor: rng.range(0, 10),
        patch: rng.range(0, 10),
    }
}

fn gen_req(rng: &mut Rng) -> Req {
    let ops = ["", "^", "~", "=", ">", ">=", "<", "<="];
    let n_comps = rng.range(1, 3);
    let mut parts = Vec::new();
    for _ in 0..n_comps {
        if rng.chance(1, 8) {
            let choice = rng.below(3);
            let s = match choice {
                0 => "*".to_string(),
                1 => format!("{}.*", rng.range(0, 5)),
                _ => format!("{}.{}.*", rng.range(0, 5), rng.range(0, 5)),
            };
            parts.push(s);
        } else {
            let op = ops[rng.below(ops.len())];
            let form = rng.below(3);
            let ver = match form {
                0 => format!("{}", rng.range(0, 10)),
                1 => format!("{}.{}", rng.range(0, 10), rng.range(0, 10)),
                _ => format!(
                    "{}.{}.{}",
                    rng.range(0, 10),
                    rng.range(0, 10),
                    rng.range(0, 10)
                ),
            };
            parts.push(format!("{op}{ver}"));
        }
    }
    Req::parse(&parts.join(", ")).unwrap()
}

#[test]
fn manifest_exact_display_and_comments() {
    let text = "\
# leading comment
  # indented comment

[package]
name = \"web-app\"
version = \"1.2.3\"

[dependencies]
http = \"^1.2\"
log = \">=0.3, <0.5\"
";
    let m = Manifest::parse(text).unwrap();
    assert_eq!(m.name, "web-app");
    assert_eq!(m.version, Version::parse("1.2.3").unwrap());
    assert_eq!(m.dependencies.len(), 2);

    let expected = "\
[package]
name = \"web-app\"
version = \"1.2.3\"

[dependencies]
http = \"^1.2\"
log = \">=0.3, <0.5\"
";
    assert_eq!(m.to_string(), expected);
    assert_eq!(Manifest::parse(&m.to_string()).unwrap(), m);

    let no_deps = Manifest {
        name: "solo".to_string(),
        version: Version::parse("0.1.0").unwrap(),
        dependencies: vec![],
    };
    assert_eq!(
        no_deps.to_string(),
        "[package]\nname = \"solo\"\nversion = \"0.1.0\"\n"
    );
    assert_eq!(Manifest::parse(&no_deps.to_string()).unwrap(), no_deps);
}

#[test]
fn manifest_error_lines() {
    let cases: &[(&str, usize)] = &[
        ("name = \"a\"\n", 1),
        ("[dependencies]\na = \"1\"\n", 1),
        (
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\n[package]\n",
            4,
        ),
        (
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\n[unknown]\n",
            4,
        ),
        (
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\nextra = \"1\"\n",
            4,
        ),
        (
            "[package]\nname = \"a\"\nname = \"b\"\nversion = \"1.0.0\"\n",
            3,
        ),
        ("[package]\nname = \"A\"\nversion = \"1.0.0\"\n", 2),
        ("[package]\nname = \"a\"\nversion = \"1.0\"\n", 3),
        ("[package]\nname = a\nversion = \"1.0.0\"\n", 2),
        (
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\n[dependencies]\nb = \"1\"\nb = \"2\"\n",
            6,
        ),
        (
            "[package]\nname = \"a\"\nversion = \"1.0.0\"\n[dependencies]\nb = \"bad\"\n",
            5,
        ),
        ("[package]\nname = \"a\"\n", 2),
        ("", 1),
    ];
    for &(input, expected_line) in cases {
        let err = Manifest::parse(input).unwrap_err();
        assert_eq!(err.line, expected_line, "wrong line for:\n{input}");
    }
}

#[test]
fn manifest_roundtrip_forall() {
    forall(200, 0xCA14_0002, |rng| {
        let name = gen_name(rng);
        let version = gen_version(rng);
        let n_deps = rng.below(7);
        let mut seen = BTreeSet::new();
        let mut dependencies = Vec::new();
        for _ in 0..n_deps {
            let d = gen_name(rng);
            if seen.insert(d.clone()) {
                dependencies.push((d, gen_req(rng)));
            }
        }
        let m = Manifest {
            name,
            version,
            dependencies,
        };
        let printed = m.to_string();
        assert_eq!(Manifest::parse(&printed).unwrap(), m);
    });
}

#[test]
fn lock_roundtrip_forall_and_errors() {
    forall(200, 0xCA14_0003, |rng| {
        let mut sol = Solution::new();
        let n = rng.below(10);
        for _ in 0..n {
            sol.insert(gen_name(rng), gen_version(rng));
        }
        let text = lock::write(&sol);
        assert!(text.starts_with("# cairn lock v1\n"));
        assert_eq!(lock::read(&text).unwrap(), sol);
    });

    // Blank lines after header are ignored
    let with_blanks = "# cairn lock v1\n\nalpha = \"1.0.0\"\n\nbeta = \"2.3.4\"\n\n";
    let parsed = lock::read(with_blanks).unwrap();
    assert_eq!(parsed.len(), 2);

    // Error cases
    assert!(lock::read("").is_err());
    assert!(lock::read("\n# cairn lock v1\n").is_err());
    assert!(lock::read("# cairn lock v2\n").is_err());
    assert!(lock::read("# cairn lock v1\n# comment\n").is_err());
    assert!(lock::read("# cairn lock v1\na = \"1.0.0\"\na = \"1.0.0\"\n").is_err());
    assert!(lock::read("# cairn lock v1\na = 1.0.0\n").is_err());
    assert!(lock::read("# cairn lock v1\nA = \"1.0.0\"\n").is_err());
    assert!(lock::read("# cairn lock v1\na = \"1.0\"\n").is_err());
}
