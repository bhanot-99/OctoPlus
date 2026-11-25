#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use octoplus_knowledge::companion::mode::{
    Mode, ModeMachine, Signals, signals_from_text, switch_instruction,
};
use octoplus_knowledge::companion::persona::Persona;

#[test]
fn test_same_seed_gives_same_mode_sequence() {
    let persona = Persona::default_packaged().unwrap();
    let mut m1 = ModeMachine::new(&persona.switching, 42);
    let mut m2 = ModeMachine::new(&persona.switching, 42);

    let sig = Signals::default();
    for _ in 0..100 {
        let d1 = m1.step(sig);
        let d2 = m2.step(sig);
        assert_eq!(d1, d2);
    }
    assert_eq!(m1.counters(), m2.counters());
}

#[test]
fn test_neutral_text_statistics_5000_turns() {
    let persona = Persona::default_packaged().unwrap();
    let mut machine = ModeMachine::new(&persona.switching, 12345);
    let sig = Signals::default();

    let mut prev_switched = false;
    for _ in 0..5000 {
        let dec = machine.step(sig);
        if prev_switched {
            assert!(
                !dec.switched,
                "never two between-turn switches in a row when never_twice_in_row is true"
            );
        }
        prev_switched = dec.switched;
    }

    let counters = machine.counters();
    assert_eq!(counters.turns, 5000);

    let total_switch_rate = (counters.switches + counters.mid_reply_switches) as f64 / 5000.0;
    assert!(
        (0.20..=0.30).contains(&total_switch_rate),
        "total switch rate must be between 0.20 and 0.30, got {total_switch_rate}"
    );

    let heart_share = counters.heart_turns as f64 / 5000.0;
    assert!(
        (0.40..=0.60).contains(&heart_share),
        "heart share must be between 0.40 and 0.60, got {heart_share}"
    );

    let mid_rate = counters.mid_reply_switches as f64 / 5000.0;
    assert!(
        (0.02..=0.08).contains(&mid_rate),
        "mid-reply switch rate must be between 0.02 and 0.08, got {mid_rate}"
    );

    // Counters add up
    assert_eq!(counters.turns, counters.heart_turns + counters.logic_turns);
    assert!(counters.switches <= counters.turns);
    assert!(counters.mid_reply_switches <= counters.turns);
}

#[test]
fn test_never_two_switches_in_row() {
    let persona = Persona::default_packaged().unwrap();
    let mut machine = ModeMachine::new(&persona.switching, 9999);
    let sig = Signals::default();

    let mut prev_switched = false;
    for _ in 0..5000 {
        let dec = machine.step(sig);
        if persona.switching.never_twice_in_row && prev_switched {
            assert!(!dec.switched, "between-turn switch occurred twice in a row");
        }
        prev_switched = dec.switched;
    }
}

#[test]
fn test_context_signals_shift_modes() {
    let persona = Persona::default_packaged().unwrap();
    let turns = 2000;

    // Neutral baseline
    let mut neutral_machine = ModeMachine::new(&persona.switching, 777);
    let neutral_sig = Signals::default();
    for _ in 0..turns {
        neutral_machine.step(neutral_sig);
    }
    let neutral_heart_share = neutral_machine.counters().heart_turns as f64 / turns as f64;
    let neutral_logic_share = neutral_machine.counters().logic_turns as f64 / turns as f64;

    // Sad text
    let mut sad_machine = ModeMachine::new(&persona.switching, 777);
    let sad_sig = signals_from_text("I feel so sad and tired today");
    assert!(sad_sig.sad_or_tired);
    for _ in 0..turns {
        sad_machine.step(sad_sig);
    }
    let sad_heart_share = sad_machine.counters().heart_turns as f64 / turns as f64;

    assert!(
        sad_heart_share - neutral_heart_share >= 0.10,
        "sad text heart share ({sad_heart_share}) must be >= neutral ({neutral_heart_share}) + 0.10"
    );

    // Plan text
    let mut plan_machine = ModeMachine::new(&persona.switching, 777);
    let plan_sig = signals_from_text("What is the plan and the steps to fix this deadline?");
    assert!(plan_sig.wants_plan_or_facts);
    for _ in 0..turns {
        plan_machine.step(plan_sig);
    }
    let plan_logic_share = plan_machine.counters().logic_turns as f64 / turns as f64;

    assert!(
        plan_logic_share > neutral_logic_share,
        "plan text logic share ({plan_logic_share}) must be higher than neutral ({neutral_logic_share})"
    );
}

#[test]
fn test_empty_text_never_panics() {
    let persona = Persona::default_packaged().unwrap();
    let mut machine = ModeMachine::new(&persona.switching, 1);
    let sig = signals_from_text("");
    assert!(!sig.sad_or_tired);
    assert!(!sig.wants_plan_or_facts);
    let dec = machine.step(sig);
    assert!(matches!(dec.mode, Mode::Logic | Mode::Heart));
}

#[test]
fn test_switch_instruction() {
    let persona = Persona::default_packaged().unwrap();
    let mut machine = ModeMachine::new(&persona.switching, 100);
    let mut found_switch = false;
    let mut found_mid = false;

    for _ in 0..200 {
        let dec = machine.step(Signals::default());
        let instr = switch_instruction(&dec);
        if dec.mid_reply_switch.is_some() {
            assert!(instr.is_some());
            assert!(instr.unwrap().contains("partway through"));
            found_mid = true;
        } else if dec.switched {
            assert!(instr.is_some());
            assert!(instr.unwrap().contains("just changed to"));
            found_switch = true;
        } else {
            assert!(instr.is_none());
        }
    }
    assert!(
        found_switch,
        "should observe a between-turn switch in 200 turns"
    );
    assert!(found_mid, "should observe a mid-reply switch in 200 turns");
}
