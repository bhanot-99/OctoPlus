//! Dump format: round trip, golden bytes, and every way a file can be wrong (FR-MON-12).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use neuroos_proto::dump::{DumpError, DumpReader, DumpWriter, END_MARK, MAGIC, MAX_EVENT_BYTES};
use neuroos_proto::v1::monitor_event::Event;
use neuroos_proto::v1::{FocusChanged, IdleChanged, MonitorEvent};
use proptest::prelude::*;

fn idle(ns: u64) -> MonitorEvent {
    MonitorEvent {
        observed_at_ns: ns,
        event: Some(Event::Idle(IdleChanged { idle: true })),
    }
}

fn written(events: &[MonitorEvent]) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut w = DumpWriter::new(&mut buf).unwrap();
    for e in events {
        w.write(e).unwrap();
    }
    assert_eq!(w.finish().unwrap(), events.len() as u64);
    buf
}

fn read_all(bytes: &[u8]) -> Result<Vec<MonitorEvent>, DumpError> {
    let mut r = DumpReader::new(bytes)?;
    let mut out = Vec::new();
    while let Some(e) = r.next_event()? {
        out.push(e);
    }
    Ok(out)
}

#[test]
fn golden_bytes_for_an_empty_and_a_one_event_dump() {
    // magic · version 1 · end mark · count 0
    let empty = written(&[]);
    let mut expect = MAGIC.to_vec();
    expect.extend_from_slice(&1u32.to_le_bytes());
    expect.extend_from_slice(&END_MARK.to_le_bytes());
    expect.extend_from_slice(&0u64.to_le_bytes());
    assert_eq!(empty, expect);

    let one = written(&[idle(7)]);
    assert_eq!(&one[..12], &expect[..12]);
    // length 6 · observed_at_ns=7 (08 07) · idle field 11 (5a 02) { idle=true (08 01) }
    assert_eq!(
        &one[12..22],
        &[6, 0, 0, 0, 0x08, 0x07, 0x5a, 0x02, 0x08, 0x01]
    );
    assert_eq!(one.len(), 12 + 10 + 4 + 8);
}

#[test]
fn round_trip_keeps_order_and_content() {
    let evs: Vec<MonitorEvent> = (1..=50).map(idle).collect();
    assert_eq!(read_all(&written(&evs)).unwrap(), evs);
}

#[test]
fn every_truncation_point_is_detected() {
    let full = written(&[idle(1), idle(2)]);
    for cut in 12..full.len() {
        let err = read_all(&full[..cut]).unwrap_err();
        assert!(
            matches!(err, DumpError::Truncated { .. } | DumpError::BadEvent),
            "cut at {cut}: {err}"
        );
    }
}

#[test]
fn bad_magic_version_length_and_count_are_rejected() {
    assert!(matches!(read_all(b"").unwrap_err(), DumpError::BadMagic));
    assert!(matches!(
        read_all(b"NOTADUMP12345").unwrap_err(),
        DumpError::BadMagic
    ));
    let mut v2 = MAGIC.to_vec();
    v2.extend_from_slice(&2u32.to_le_bytes());
    assert!(matches!(
        read_all(&v2).unwrap_err(),
        DumpError::UnsupportedVersion(2)
    ));

    let mut huge = written(&[]);
    huge.splice(12..16, (MAX_EVENT_BYTES + 1).to_le_bytes());
    assert!(matches!(
        read_all(&huge).unwrap_err(),
        DumpError::TooLarge(_)
    ));

    let mut wrong = written(&[idle(1)]);
    let n = wrong.len();
    wrong[n - 8..].copy_from_slice(&5u64.to_le_bytes());
    assert!(matches!(
        read_all(&wrong).unwrap_err(),
        DumpError::CountMismatch {
            declared: 5,
            found: 1
        }
    ));
}

proptest! {
    #[test]
    fn arbitrary_bytes_never_panic_the_reader(bytes in proptest::collection::vec(any::<u8>(), 0..200)) {
        let _ = read_all(&bytes);
        let mut with_header = MAGIC.to_vec();
        with_header.extend_from_slice(&1u32.to_le_bytes());
        with_header.extend_from_slice(&bytes);
        let _ = read_all(&with_header);
    }

    #[test]
    fn any_titles_round_trip(titles in proptest::collection::vec(".{0,40}", 0..20)) {
        let evs: Vec<MonitorEvent> = titles.iter().enumerate().map(|(i, t)| MonitorEvent {
            observed_at_ns: i as u64,
            event: Some(Event::Focus(FocusChanged { title: t.clone(), ..Default::default() })),
        }).collect();
        prop_assert_eq!(read_all(&written(&evs)).unwrap(), evs);
    }
}
