#![allow(clippy::unwrap_used)]
//! Tests of the memory store: adding, dedup, search and the CLI-independent helpers.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use aci::envelope::Code;
use monolith::memory::{score, words, Item, Kind, Store};

/// A unique folder under the temp dir, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "monolith-memory-{}-{name}-{nanos}",
            std::process::id()
        ));
        Self(dir)
    }

    fn store(&self) -> Store {
        Store::at(&self.0)
    }

    fn file_text(&self) -> String {
        fs::read_to_string(self.0.join("items.jsonl")).unwrap_or_default()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn texts(items: &[Item]) -> Vec<&str> {
    items.iter().map(|i| i.text.as_str()).collect()
}

fn ids(items: &[Item]) -> Vec<u64> {
    items.iter().map(|i| i.id).collect()
}

#[test]
fn add_and_list_round_trip_keeps_ids_and_kinds() {
    let s = Scratch::new("round-trip");
    let store = s.store();
    assert_eq!(
        store
            .add(Kind::Fact, "the server uses port 8473", false)
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .add(Kind::Lesson, "check status before commit", false)
            .unwrap(),
        2
    );
    assert_eq!(
        store
            .add(Kind::Skill, "init then add then commit", true)
            .unwrap(),
        3
    );
    let items = store.list().unwrap();
    assert_eq!(ids(&items), vec![1, 2, 3]);
    assert_eq!(
        items.iter().map(|i| i.kind).collect::<Vec<_>>(),
        vec![Kind::Fact, Kind::Lesson, Kind::Skill]
    );
    assert_eq!(items[0].text, "the server uses port 8473");
    assert!(!items[0].verified);
    assert!(items[2].verified);
    assert!(items[0].ts_ms > 0);
}

#[test]
fn kind_parse_takes_three_words_only() {
    assert_eq!(Kind::parse("fact").unwrap(), Kind::Fact);
    assert_eq!(Kind::parse("lesson").unwrap(), Kind::Lesson);
    assert_eq!(Kind::parse("skill").unwrap(), Kind::Skill);
    let e = Kind::parse("note").unwrap_err();
    assert_eq!(e.code, Code::Invalid);
    assert!(e.hint.unwrap().contains("fact, lesson, skill"));
}

#[test]
fn whitespace_is_collapsed_and_trimmed() {
    let s = Scratch::new("whitespace");
    let store = s.store();
    store
        .add(Kind::Fact, "  alpha\n\n  beta\tgamma  \n", false)
        .unwrap();
    assert_eq!(texts(&store.list().unwrap()), vec!["alpha beta gamma"]);
}

#[test]
fn duplicate_returns_same_id_and_does_not_grow_the_file() {
    let s = Scratch::new("dedup");
    let store = s.store();
    assert_eq!(store.add(Kind::Fact, "Hello World", false).unwrap(), 1);
    assert_eq!(store.add(Kind::Fact, "hello   world", false).unwrap(), 1);
    assert_eq!(store.list().unwrap().len(), 1);
    assert_eq!(s.file_text().lines().count(), 1);
    // Same text under another kind is a different item.
    assert_eq!(store.add(Kind::Lesson, "Hello World", false).unwrap(), 2);
}

#[test]
fn verified_duplicate_promotes_the_stored_item() {
    let s = Scratch::new("promote");
    let store = s.store();
    assert_eq!(
        store.add(Kind::Lesson, "use status first", false).unwrap(),
        1
    );
    assert_eq!(store.add(Kind::Lesson, "other lesson", false).unwrap(), 2);
    assert_eq!(s.file_text().lines().count(), 2);
    assert_eq!(
        store.add(Kind::Lesson, "USE status first", true).unwrap(),
        1
    );
    let items = store.list().unwrap();
    assert!(items[0].verified);
    assert!(!items[1].verified);
    assert_eq!(s.file_text().lines().count(), 2);
    assert!(!s.0.join("items.jsonl.tmp").exists());
}

#[test]
fn empty_and_too_long_text_is_refused() {
    let s = Scratch::new("refuse");
    let store = s.store();
    let empty = store.add(Kind::Fact, " \n\t ", false).unwrap_err();
    assert_eq!(empty.code, Code::Invalid);
    let long = "a".repeat(2001);
    let too_long = store.add(Kind::Fact, &long, false).unwrap_err();
    assert_eq!(too_long.code, Code::Invalid);
    assert!(too_long.hint.unwrap().contains("one idea"));
    assert_eq!(store.add(Kind::Fact, &"a".repeat(2000), false).unwrap(), 1);
}

#[test]
fn missing_store_lists_and_searches_empty() {
    let s = Scratch::new("missing");
    let store = s.store();
    assert!(store.list().unwrap().is_empty());
    assert!(store.search("anything", 5, &[], false).unwrap().is_empty());
}

#[test]
fn garbage_lines_are_skipped() {
    let s = Scratch::new("garbage");
    let store = s.store();
    store.add(Kind::Fact, "first fact", false).unwrap();
    let mut text = s.file_text();
    text.push_str("this is not json\n{\"id\":\"x\",\"kind\":\"fact\"}\n\n");
    fs::write(s.0.join("items.jsonl"), text).unwrap();
    let items = store.list().unwrap();
    assert_eq!(texts(&items), vec!["first fact"]);
    assert_eq!(store.add(Kind::Fact, "second fact", false).unwrap(), 2);
    assert_eq!(
        texts(&store.list().unwrap()),
        vec!["first fact", "second fact"]
    );
}

#[test]
fn words_drop_stop_words_strip_plurals_and_split_punctuation() {
    assert_eq!(
        words("The chronos logs are in the fsck list, ls!"),
        vec!["chrono", "log", "fsck", "list", "ls"]
    );
    assert_eq!(words("Fsck-Run_NOW!"), vec!["fsck", "run", "now"]);
    assert_eq!(words("this is it"), Vec::<String>::new());
    assert_eq!(words("hellos hello"), vec!["hello"]);
    assert_eq!(words("its a at"), vec!["its"]);
    assert_eq!(words("status lacks"), vec!["statu", "lack"]);
    assert_eq!(words("café 日本"), vec!["caf"]);
}

#[test]
fn score_counts_shared_words() {
    assert_eq!(score("fsck", "chronos status"), 0.0);
    assert_eq!(score("", "chronos"), 0.0);
    assert_eq!(score("chronos", ""), 0.0);
    let two = score("chronos log", "the chronos log");
    let one = score("chronos log", "chronos only");
    assert!(two > one);
    assert!(one > 0.0);
}

#[test]
fn search_ranks_a_rare_shared_word_above_a_common_one() {
    let s = Scratch::new("rare");
    let store = s.store();
    for text in [
        "repo keeps a readme",
        "repo keeps history",
        "repo has tests",
        "repo has a license",
        "repo uses git",
        "repo needs a build step",
        "repo lists its files",
        "zebra crossing",
    ] {
        store.add(Kind::Fact, text, false).unwrap();
    }
    let hits = store.search("repo zebra", 2, &[], false).unwrap();
    assert_eq!(ids(&hits), vec![8, 7]);
}

#[test]
fn search_filters_by_kind() {
    let s = Scratch::new("kinds");
    let store = s.store();
    store.add(Kind::Fact, "zebra fact", false).unwrap();
    store.add(Kind::Lesson, "zebra lesson", false).unwrap();
    store.add(Kind::Skill, "zebra skill", false).unwrap();
    let lessons = store.search("zebra", 5, &[Kind::Lesson], false).unwrap();
    assert_eq!(texts(&lessons), vec!["zebra lesson"]);
    let two = store
        .search("zebra", 5, &[Kind::Fact, Kind::Skill], false)
        .unwrap();
    assert_eq!(two.len(), 2);
    assert_eq!(store.search("zebra", 5, &[], false).unwrap().len(), 3);
}

#[test]
fn verified_only_hides_unverified_items() {
    let s = Scratch::new("verified-only");
    let store = s.store();
    store.add(Kind::Lesson, "zebra one", false).unwrap();
    store.add(Kind::Lesson, "zebra two", true).unwrap();
    let all = store.search("zebra", 5, &[], false).unwrap();
    assert_eq!(all.len(), 2);
    let verified = store.search("zebra", 5, &[], true).unwrap();
    assert_eq!(texts(&verified), vec!["zebra two"]);
}

#[test]
fn k_limits_the_hits_and_zero_gives_none() {
    let s = Scratch::new("k");
    let store = s.store();
    for n in 0..5 {
        store
            .add(Kind::Fact, &format!("zebra number {n}"), false)
            .unwrap();
    }
    assert_eq!(store.search("zebra", 2, &[], false).unwrap().len(), 2);
    assert!(store.search("zebra", 0, &[], false).unwrap().is_empty());
}

#[test]
fn ties_put_verified_first_then_newer_first() {
    let s = Scratch::new("ties");
    let store = s.store();
    store.add(Kind::Fact, "alpha beta", false).unwrap();
    store.add(Kind::Fact, "beta alpha", false).unwrap();
    store.add(Kind::Fact, "alpha, beta", true).unwrap();
    store.add(Kind::Fact, "beta, alpha", true).unwrap();
    let hits = store.search("alpha beta", 10, &[], false).unwrap();
    assert_eq!(ids(&hits), vec![4, 3, 2, 1]);
}

#[test]
fn special_characters_survive_a_round_trip() {
    let s = Scratch::new("special");
    let store = s.store();
    let text = "He said \"hi\" \\ C:\\temp é 日本\nnext line";
    store.add(Kind::Fact, text, false).unwrap();
    let reopened = Store::at(&s.0).list().unwrap();
    assert_eq!(
        reopened[0].text,
        "He said \"hi\" \\ C:\\temp é 日本 next line"
    );
}
