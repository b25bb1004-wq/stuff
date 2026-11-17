#![allow(clippy::unwrap_used)]
//! Door device through the public machine API: frames, property round trip, snapshot.
use assay::forall;
use silicon::machine::{Config, Machine};
use silicon::Bus;

const RAM: u64 = 0x8000_0000;
const DOOR: u64 = 0x1000_4000;

fn machine() -> Machine {
    Machine::new(Config {
        ram_size: 64 * 1024,
        ..Config::default()
    })
}

#[test]
fn host_frames_arrive_in_order_for_random_inputs() {
    forall(200, 0xD00D, |r| {
        let mut m = machine();
        let n = 1 + r.below(8);
        let mut sent = Vec::new();
        for _ in 0..n {
            let chan = r.u64() as u8;
            let len = 1 + r.below(300);
            let data: Vec<u8> = (0..len).map(|_| r.u64() as u8).collect();
            m.door_push_to_guest(chan, &data);
            sent.push((chan, data));
        }
        for (chan, data) in sent {
            let len = data.len() as u64;
            assert_eq!(m.board.read(DOOR + 0x38, 8).unwrap(), len);
            assert_eq!(m.board.read(DOOR + 0x40, 8).unwrap(), u64::from(chan));
            m.board.write(DOOR + 0x20, 8, RAM + 0x1000).unwrap();
            m.board.write(DOOR + 0x28, 8, 4096).unwrap();
            m.board.write(DOOR + 0x48, 8, 1).unwrap();
            let got: Vec<u8> = (0..len)
                .map(|i| m.board.read(RAM + 0x1000 + i, 1).unwrap() as u8)
                .collect();
            assert_eq!(got, data);
        }
        assert_eq!(m.board.read(DOOR + 0x30, 8).unwrap() & 1, 0);
        assert_eq!(m.door_dropped(), 0);
    });
}

#[test]
fn guest_frame_round_trip_over_bus_writes() {
    let mut m = machine();
    m.load(RAM + 0x2000, b"guest says hi").unwrap();
    m.board.write(DOOR, 8, RAM + 0x2000).unwrap();
    m.board.write(DOOR + 0x08, 8, 13).unwrap();
    m.board.write(DOOR + 0x10, 8, 200).unwrap();
    m.board.write(DOOR + 0x18, 8, 1).unwrap();
    assert_eq!(m.board.read(DOOR + 0x30, 4).unwrap() & 2, 0);
    assert_eq!(
        m.door_take_from_guest(),
        vec![(200u8, b"guest says hi".to_vec())]
    );
    assert!(m.door_take_from_guest().is_empty());
}

#[test]
fn snapshot_keeps_queued_frames_both_ways() {
    let mut m = machine();
    m.door_push_to_guest(9, b"first");
    m.door_push_to_guest(2, b"second");
    m.load(RAM + 0x2000, b"out").unwrap();
    m.board.write(DOOR, 8, RAM + 0x2000).unwrap();
    m.board.write(DOOR + 0x08, 8, 3).unwrap();
    m.board.write(DOOR + 0x18, 8, 1).unwrap();

    let snap = m.snapshot(None);
    let mut n = machine();
    n.restore(&[&snap]).unwrap();

    assert_eq!(n.board.read(DOOR + 0x38, 8).unwrap(), 5);
    assert_eq!(n.board.read(DOOR + 0x40, 8).unwrap(), 9);
    assert_eq!(n.door_take_from_guest(), vec![(0u8, b"out".to_vec())]);
    n.board.write(DOOR + 0x20, 8, RAM + 0x1000).unwrap();
    n.board.write(DOOR + 0x28, 8, 64).unwrap();
    n.board.write(DOOR + 0x48, 8, 1).unwrap();
    assert_eq!(n.board.read(DOOR + 0x38, 8).unwrap(), 6);
    assert_eq!(n.board.read(DOOR + 0x40, 8).unwrap(), 2);
}
