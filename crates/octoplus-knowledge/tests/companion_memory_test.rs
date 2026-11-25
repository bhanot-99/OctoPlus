#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Memory of the owner (story C-S05): the chance rule averages 40 to 50 percent, the exclusions
//! are never saved, ambient speech is never saved, and what she says about her memory is plain.

use std::collections::HashMap;

use octoplus_knowledge::companion::guards;
use octoplus_knowledge::companion::memory::{
    Category, Decision, Exclusion, MemoryRule, NOTICED_MARKER, Skip, asks_what_i_remember,
    exclusion_of, memory_list_line, plain_form, spoken_form,
};
use octoplus_knowledge::companion::persona::Persona;

fn persona() -> Persona {
    Persona::default_packaged().unwrap()
}

/// Personal statements an owner might make, in every category.
fn statements() -> Vec<(&'static str, Category)> {
    vec![
        ("My name is Arjun and I go by Arj", Category::Identity),
        ("You can call me Arj", Category::Identity),
        ("I have an interview on Thursday at ten", Category::Plan),
        ("I'm going to visit my parents next week", Category::Plan),
        ("My flight to Pune leaves tomorrow morning", Category::Plan),
        ("I have a presentation on Monday", Category::Plan),
        ("I feel a bit nervous about it", Category::Feeling),
        ("I'm feeling really proud of the report", Category::Feeling),
        ("I'm excited about the weekend", Category::Feeling),
        ("My dog Rex hates the vacuum cleaner", Category::People),
        ("My sister just moved to Berlin", Category::People),
        ("My boss changed the schedule again", Category::People),
        ("I work on compilers at a small company", Category::Life),
        ("I live near the river", Category::Life),
        ("My birthday is in March", Category::Life),
        (
            "I love strong black tea in the evening",
            Category::Preference,
        ),
        ("I prefer quiet mornings", Category::Preference),
        ("My favourite season is autumn", Category::Preference),
        ("I never eat breakfast before ten", Category::Preference),
        ("I hate loud restaurants", Category::Preference),
    ]
}

#[test]
fn every_statement_in_the_corpus_is_recognised_as_personal() {
    let mut rule = MemoryRule::new(&persona(), 1);
    for (text, cat) in statements() {
        // Run until it is kept once, to see its category.
        let mut seen = None;
        for _ in 0..200 {
            if let Decision::Save(k) = rule.decide(text, true) {
                seen = Some(k.category);
                break;
            }
        }
        assert_eq!(seen, Some(cat), "{text}");
    }
}

#[test]
fn the_long_run_share_stays_between_forty_and_fifty_percent() {
    let st = statements();
    for seed in [1u64, 2, 3, 99, 12345] {
        let mut rule = MemoryRule::new(&persona(), seed);
        let mut chooser = octoplus_knowledge::companion::rng::Rng::new(seed + 7);
        let n = 6000;
        for _ in 0..n {
            let (t, _) = st[chooser.pick(st.len())];
            let _ = rule.decide(t, true);
        }
        let share = rule.saved_share();
        println!("seed {seed}: {share:.3} saved of {n} personal statements");
        assert!((0.40..=0.50).contains(&share), "seed {seed}: {share}");
    }
}

#[test]
fn the_share_stays_in_band_for_any_mix_of_statements() {
    // Only plans and names (high weight), and only tastes (low weight): the controller keeps both
    // inside the owner's band.
    let high: Vec<&str> = statements()
        .into_iter()
        .filter(|(_, c)| matches!(c, Category::Identity | Category::Plan))
        .map(|(t, _)| t)
        .collect();
    let low: Vec<&str> = statements()
        .into_iter()
        .filter(|(_, c)| matches!(c, Category::Preference))
        .map(|(t, _)| t)
        .collect();
    for (name, set) in [("high", high), ("low", low)] {
        let mut rule = MemoryRule::new(&persona(), 4);
        let mut chooser = octoplus_knowledge::companion::rng::Rng::new(5);
        for _ in 0..6000 {
            let _ = rule.decide(set[chooser.pick(set.len())], true);
        }
        let share = rule.saved_share();
        println!("{name} mix: {share:.3}");
        assert!((0.40..=0.50).contains(&share), "{name}: {share}");
    }
}

#[test]
fn names_and_plans_are_kept_more_often_early_on_than_tastes() {
    // Before the controller has history, the weights alone order the categories.
    let mut by_cat: HashMap<Category, (u32, u32)> = HashMap::new();
    for seed in 0..400u64 {
        let mut rule = MemoryRule::new(&persona(), seed);
        for (t, c) in statements() {
            let e = by_cat.entry(c).or_default();
            e.1 += 1;
            if matches!(rule.decide(t, true), Decision::Save(_)) {
                e.0 += 1;
            }
        }
    }
    let share = |c: Category| {
        let (k, n) = by_cat[&c];
        f64::from(k) / f64::from(n)
    };
    assert!(share(Category::Identity) > share(Category::Preference) + 0.2);
    assert!(share(Category::Plan) > share(Category::Preference) + 0.2);
}

/// Sentences that must NEVER be kept by chance, whatever the dice say.
fn excluded() -> Vec<(&'static str, Exclusion)> {
    vec![
        (
            "My password is hunter two and I use it everywhere",
            Exclusion::Secret,
        ),
        ("I wrote my passcode on a sticky note", Exclusion::Secret),
        (
            "My api key starts with sk and then some letters",
            Exclusion::Secret,
        ),
        ("I got an otp from the bank just now", Exclusion::Secret),
        (
            "My recovery phrase is in a drawer at home",
            Exclusion::Secret,
        ),
        ("My credit card number is on the table", Exclusion::Number),
        (
            "I read my account number as 12345678901 to them",
            Exclusion::Number,
        ),
        ("My phone is 98765 43210 if you need it", Exclusion::Number),
        ("My iban is saved in my notes", Exclusion::Number),
        ("My aadhaar card is expired I think", Exclusion::Number),
        (
            "I was diagnosed with something last year",
            Exclusion::Health,
        ),
        ("I take medication every morning", Exclusion::Health),
        ("My doctor said to rest more", Exclusion::Health),
        ("I started therapy last month", Exclusion::Health),
        ("I have diabetes in my family", Exclusion::Health),
        ("I am allergic to peanuts", Exclusion::Health),
        ("I had surgery on my knee", Exclusion::Health),
        (
            "My friend told me in confidence that she quit",
            Exclusion::OthersPrivate,
        ),
        (
            "Between us my boss is cheating on his wife",
            Exclusion::OthersPrivate,
        ),
        (
            "My sister got divorced and asked me not to tell",
            Exclusion::OthersPrivate,
        ),
        (
            "Do not tell anyone but my brother is in debt",
            Exclusion::OthersPrivate,
        ),
        ("I want to kill myself and nobody knows", Exclusion::Crisis),
        ("I don't want to be alive tonight", Exclusion::Crisis),
    ]
}

#[test]
fn nothing_excluded_is_ever_kept_by_chance() {
    let mut rule = MemoryRule::new(&persona(), 3);
    for (text, why) in excluded() {
        assert_eq!(exclusion_of(text, &[]), Some(why), "{text}");
        for _ in 0..2000 {
            match rule.decide(text, true) {
                Decision::Save(_) => panic!("saved an excluded sentence: {text}"),
                Decision::Skip(Skip::Excluded(w)) => assert_eq!(w, why, "{text}"),
                Decision::Skip(other) => panic!("{text}: skipped for {other:?}, not the exclusion"),
            }
        }
    }
    assert!(rule.counters().excluded >= 2000 * excluded().len() as u64);
    assert_eq!(rule.counters().saved, 0);
}

#[test]
fn a_topic_the_owner_marked_sensitive_is_never_kept() {
    let mut p = persona();
    p.avoid_topics = vec!["my ex".to_string(), "politics".to_string()];
    let mut rule = MemoryRule::new(&p, 8);
    for text in [
        "I still think about my ex a lot these days",
        "I get tired of politics at family dinners",
    ] {
        for _ in 0..1000 {
            assert!(
                !matches!(rule.decide(text, true), Decision::Save(_)),
                "{text}"
            );
        }
        assert_eq!(
            exclusion_of(text, &p.avoid_topics),
            Some(Exclusion::SensitiveTopic)
        );
    }
    // The same kind of sentence without the topic is still kept sometimes.
    let mut kept = 0;
    for _ in 0..200 {
        if matches!(
            rule.decide("I love loud dinners at home", true),
            Decision::Save(_)
        ) {
            kept += 1;
        }
    }
    assert!(kept > 0);
}

#[test]
fn ambient_speech_is_never_kept_even_when_it_sounds_personal() {
    let mut rule = MemoryRule::new(&persona(), 6);
    for (text, _) in statements() {
        for _ in 0..500 {
            assert_eq!(
                rule.decide(text, false),
                Decision::Skip(Skip::Ambient),
                "{text}"
            );
        }
    }
    assert_eq!(rule.counters().saved, 0);
    assert_eq!(rule.counters().considered, 0);
}

#[test]
fn questions_commands_and_chatter_are_not_facts() {
    let mut rule = MemoryRule::new(&persona(), 6);
    for text in [
        "What is my name again?",
        "Do I have a meeting tomorrow",
        "Can you tell me something fun",
        "Tell me about my day",
        "Play some music please",
        "hmm okay",
        "yes",
        "The weather is nice today",
        "That movie was long",
        "Why is my laptop so slow",
        "I",
    ] {
        for _ in 0..300 {
            assert!(
                !matches!(rule.decide(text, true), Decision::Save(_)),
                "kept: {text}"
            );
        }
    }
    // Far too long.
    let long = format!("I think {}", "very ".repeat(60));
    assert_eq!(rule.decide(&long, true), Decision::Skip(Skip::WrongSize));
}

#[test]
fn what_is_kept_is_marked_and_shown_plainly() {
    let mut rule = MemoryRule::new(&persona(), 2);
    let mut keep = None;
    for _ in 0..100 {
        if let Decision::Save(k) = rule.decide("My dog Rex hates the vacuum cleaner.", true) {
            keep = Some(k);
            break;
        }
    }
    let k = keep.unwrap();
    assert_eq!(
        k.fact,
        format!("{NOTICED_MARKER}My dog Rex hates the vacuum cleaner")
    );
    assert!(k.fact.len() < 512);
    assert_eq!(plain_form(&k.fact), "My dog Rex hates the vacuum cleaner");
    assert_eq!(
        spoken_form(&k.fact),
        "I noticed: My dog Rex hates the vacuum cleaner"
    );
    // A fact the owner asked for is shown as it is.
    assert_eq!(spoken_form("my wifi name is Blue"), "my wifi name is Blue");
}

#[test]
fn the_got_it_line_comes_up_now_and_then_not_always() {
    let mut rule = MemoryRule::new(&persona(), 21);
    let (mut saved, mut acked) = (0, 0);
    for _ in 0..4000 {
        if let Decision::Save(k) = rule.decide("I love strong black tea in the evening", true) {
            saved += 1;
            acked += u32::from(k.acknowledge);
        }
    }
    let share = f64::from(acked) / f64::from(saved);
    println!("got-it share {share:.2}");
    assert!((0.25..=0.45).contains(&share), "{share}");
}

#[test]
fn what_do_you_remember_is_understood_and_listed_plainly() {
    for t in [
        "what do you remember",
        "What do you remember about me?",
        "what do you know about me",
        "tell me what you remember",
        "what have you noticed about me",
    ] {
        assert!(asks_what_i_remember(t), "{t}");
    }
    for t in [
        "what do you remember about the movie we talked about last night in detail",
        "do you remember",
        "what is the capital of France",
        "remember that I like tea",
        "I remember my childhood house",
    ] {
        assert!(!asks_what_i_remember(t), "{t}");
    }
    assert!(memory_list_line(&[]).contains("not holding on to anything"));
    let facts: Vec<String> = vec![
        "I like tea".to_string(),
        format!("{NOTICED_MARKER}my dog is Rex"),
    ];
    let line = memory_list_line(&facts);
    assert!(line.contains("I like tea"));
    assert!(line.contains("I noticed: my dog is Rex"));
    assert!(!line.contains("(noticed)"));
    assert!(line.contains("forget that"));
    let many: Vec<String> = (0..8).map(|i| format!("fact number {i}")).collect();
    assert!(memory_list_line(&many).contains("And 3 more"));
    assert!(guards::forbidden_phrase(&line).is_none());
}

#[test]
fn same_seed_same_decisions() {
    let run = |seed| {
        let mut rule = MemoryRule::new(&persona(), seed);
        (0..300)
            .map(|i| {
                let (t, _) = statements()[i % statements().len()];
                matches!(rule.decide(t, true), Decision::Save(_))
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(run(5), run(5));
    assert_ne!(run(5), run(6));
}
