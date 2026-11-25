#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Directedness tests for companion open-mic sessions (Phase 12, story C-S02c).
//!
//! Evaluates that owner speech addressed to Laila is reliably answered (>= 0.95),
//! while background television, radio, commentary, ambient noise, and unaddressed talk
//! are silently ignored (>= 0.90).

use octoplus_knowledge::companion::directed::{self, Directed, IgnoreReason};

struct OwnerCase {
    text: &'static str,
    last_reply_was_question: bool,
    session_turns: u32,
}

const OWNER_CORPUS: &[OwnerCase] = &[
    // 1-28: Questions, interrogatives, modals
    OwnerCase {
        text: "What time is it right now?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Why did that command fail?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "How do I fix this compiler error?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Where did I save that document?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "When is our next calendar event?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Who sent that email earlier?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Which file should I modify first?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Can you help me understand this paragraph?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Could you look at this code snippet?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Do you think we should refactor this?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Are you still listening?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Will you check the test results?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Would you explain this error message?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Can I ask a quick question?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Should we rerun the benchmark test?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Is there any memory leak in that function?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Is it going to rain this afternoon?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Have you seen any issues with that service?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Did you notice the timeout setting?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Tell me about the design pattern used here.",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "What do you think about this architecture?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Why does this test fail intermittently?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "How should I structure the new module?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Can you summarize the main findings?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Could you check if the service is running?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Are you ready for the next step?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Do you know what caused the crash?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Will you remind me to commit before leaving?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    // 29-40: Direct name mentions ("Laila", "Jarvis") and direct address ("you", "your")
    OwnerCase {
        text: "Laila, what do you think?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Hey Laila, are you there?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Laila, help me out here.",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Jarvis, what is the status?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Hey Jarvis, can you check this?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "I think you made a good point earlier.",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "You are being really helpful today.",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "What was your recommendation again?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "I want your honest opinion on this.",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Take your time with that reply.",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Can you hear me clearly?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "I appreciate your quick help.",
        last_reply_was_question: false,
        session_turns: 0,
    },
    // 41-50: Goodbyes
    OwnerCase {
        text: "Goodbye Laila, talk tomorrow.",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Good night, going to sleep now.",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Bye for now, see you later.",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Talk to you later today.",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "See you tomorrow morning.",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "I have to head out now, goodbye.",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Goodnight Laila.",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Catch you later.",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "I gotta go, talk later.",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Signing off for the night, bye.",
        last_reply_was_question: false,
        session_turns: 0,
    },
    // 51-55: Crisis phrases
    OwnerCase {
        text: "I want to end my life.",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "I feel like I want to die.",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "I am thinking about suicide.",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "I want to kill myself.",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "Is there a suicide hotline?",
        last_reply_was_question: false,
        session_turns: 0,
    },
    // 56-65: Short answers to previous assistant question (<=8 words)
    OwnerCase {
        text: "Yes please.",
        last_reply_was_question: true,
        session_turns: 4,
    },
    OwnerCase {
        text: "No, not really.",
        last_reply_was_question: true,
        session_turns: 4,
    },
    OwnerCase {
        text: "Maybe later this evening.",
        last_reply_was_question: true,
        session_turns: 4,
    },
    OwnerCase {
        text: "Sure, let us do that.",
        last_reply_was_question: true,
        session_turns: 4,
    },
    OwnerCase {
        text: "Exactly that one.",
        last_reply_was_question: true,
        session_turns: 4,
    },
    OwnerCase {
        text: "I think so.",
        last_reply_was_question: true,
        session_turns: 4,
    },
    OwnerCase {
        text: "Not quite yet.",
        last_reply_was_question: true,
        session_turns: 4,
    },
    OwnerCase {
        text: "Sounds good to me.",
        last_reply_was_question: true,
        session_turns: 4,
    },
    OwnerCase {
        text: "Definitely yes.",
        last_reply_was_question: true,
        session_turns: 4,
    },
    OwnerCase {
        text: "No thank you.",
        last_reply_was_question: true,
        session_turns: 4,
    },
    // 66-70: First-person statements during session opening (turns < 3, 3..=40 words)
    OwnerCase {
        text: "I had a really exhausting day at work today.",
        last_reply_was_question: false,
        session_turns: 0,
    },
    OwnerCase {
        text: "My back hurts from sitting at my desk all afternoon.",
        last_reply_was_question: false,
        session_turns: 1,
    },
    OwnerCase {
        text: "I managed to finish the report before the deadline.",
        last_reply_was_question: false,
        session_turns: 1,
    },
    OwnerCase {
        text: "I am feeling much better after taking a walk outside.",
        last_reply_was_question: false,
        session_turns: 2,
    },
    OwnerCase {
        text: "My evening has been pretty calm and relaxing so far.",
        last_reply_was_question: false,
        session_turns: 2,
    },
];

const AMBIENT_CORPUS: &[&str] = &[
    // 1-7: Single fillers & vocal noise
    "uh",
    "um",
    "hmm",
    "mm",
    "ah",
    "er",
    "oh",
    // 8-17: Sports commentary & broadcast speech
    "The football striker passes the ball across the penalty box to the right wing.",
    "Real Madrid maintains heavy possession in the second half of the match.",
    "The referee signals three additional minutes of stoppage time before the final whistle.",
    "A fantastic header into the top corner puts the away team ahead by two goals.",
    "The defender clears the ball into the stands to relieve the pressure.",
    "The goalkeeper dives full length to push the shot around the left post.",
    "Both managers made tactical substitutions during the second half drinks break.",
    "The midfielder received a yellow card for that sliding challenge in midfield.",
    "Corner kick awarded after deflection off the full back near the corner flag.",
    "The stadium crowd erupts into cheers as the captain scores the winning goal.",
    // 18-27: News bulletins, traffic & weather reports
    "Temperatures across the region are expected to drop significantly overnight.",
    "The morning commute saw heavy traffic delays along the northbound freeway.",
    "Financial markets closed slightly higher following the central bank press conference.",
    "Heavy thunderstorms brought localized flash flooding to low-lying coastal roads.",
    "Flight forty two from Denver has landed twenty minutes behind schedule.",
    "National meteorological services issued a frost warning for agricultural counties.",
    "International diplomatic talks will resume tomorrow afternoon in Vienna.",
    "Crude oil futures traded steady amid ongoing shipping disruptions.",
    "The latest economic census indicates steady growth in manufacturing sectors.",
    "Commuter rail services between both cities will experience weekend track maintenance.",
    // 28-37: Advertisements & promotional announcements
    "Save up to fifty percent during our annual weekend clearance blowout.",
    "Visit our regional branches today for exclusive promotional incentives.",
    "Terms and conditions apply to all zero interest financing options.",
    "Available exclusively at participating grocery store locations nationwide.",
    "Upgrade to ultra high speed gigabit broadband with zero activation fees.",
    "Farm fresh seasonal organic produce delivered direct to customer doorsteps weekly.",
    "Experience the rich authentic taste of slow roasted artisan coffee beans.",
    "Tropical vacation resort packages sell out quickly during peak seasons.",
    "Premium stainless steel cookware designed with superior heat conductivity.",
    "Contact our certified customer support representatives for product assistance.",
    // 38-69: Third-person background chatter & overheard speech
    "Dinner is sitting warm on the kitchen table.",
    "The front door was left slightly unlatched by the wind.",
    "The orange cat knocked over the ceramic flower vase again.",
    "The courier driver dropped the cardboard package near the garage.",
    "The washing machine cycle finished about fifteen minutes ago.",
    "The kettle on the kitchen counter has started boiling.",
    "The dishwasher requires another detergent pod before running the rinse cycle.",
    "Municipal recycling pickup happens early every Thursday morning.",
    "Someone left the garage keys on the hallway sideboard.",
    "The fresh groceries are placed near the refrigerator door.",
    "The desktop printer ran out of cyan cartridge during printing.",
    "The central heating radiators make a soft clicking noise when cooling.",
    "The patio door needs to be firmly secured before leaving.",
    "The local supermarket closes at nine o'clock on Sunday evenings.",
    "The car headlights were left running in the apartment driveway.",
    "The television volume is set too loud for this late hour.",
    "The bathroom fan has been running continuously since breakfast.",
    "The kitchen trash can is completely full and needs replacement.",
    "The bus schedule shows the next departure in approximately twenty minutes.",
    "The pharmacy down the street doesn't open until ten tomorrow.",
    "The elevator down to the parking garage is currently under maintenance.",
    "The package tracking notification indicates delivery before sunset.",
    "The upstairs neighbors are vacuuming their living room carpet again.",
    "The street lamps outside turned on as twilight settled over the city.",
    "The local library branch offers free computer workshops every Wednesday.",
    "The garden sprinkler system activates automatically at sunrise.",
    "The postal carrier already delivered the afternoon mail bundle.",
    "The bakery down the block sells warm sourdough bread every morning.",
    "The traffic lights at the main intersection switch to flashing yellow late at night.",
    "The weather forecast predicts scattered showers throughout Sunday afternoon.",
    "The office building cafeteria closes during state holidays.",
    "The apartment smoke detector started chirping because the battery is low.",
    // 70: Monologue longer than 40 words (reading/external presentation)
    "The international maritime treaty signed between both sovereign nations established permanent continental shelf boundaries along the fifty-fourth parallel and guaranteed reciprocal navigation rights across busy commercial sea lanes throughout both winter and summer seasons without requiring compulsory naval escorts or special customs declaration paperwork.",
];

#[test]
fn test_owner_corpus_directed_response_rate() {
    assert_eq!(
        OWNER_CORPUS.len(),
        70,
        "Owner corpus must contain 70 test cases"
    );
    let mut responded = 0;
    for case in OWNER_CORPUS {
        let res = directed::assess(case.text, case.last_reply_was_question, case.session_turns);
        if res == Directed::Respond {
            responded += 1;
        } else {
            eprintln!("Owner line unexpectedly ignored: {:?}", case.text);
        }
    }

    let rate = responded as f64 / OWNER_CORPUS.len() as f64;
    println!(
        "measured owner directed response rate: {} / {} ({:.1}%)",
        responded,
        OWNER_CORPUS.len(),
        rate * 100.0
    );
    assert!(
        rate >= 0.95,
        "Owner response rate {rate:.2} fell below 0.95 threshold ({responded}/{})",
        OWNER_CORPUS.len()
    );
}

#[test]
fn test_ambient_corpus_ignored_rate() {
    assert_eq!(
        AMBIENT_CORPUS.len(),
        70,
        "Ambient corpus must contain 70 test cases"
    );
    let mut ignored = 0;
    for &line in AMBIENT_CORPUS {
        let res = directed::assess(line, false, 0);
        match res {
            Directed::Ignore(_) => ignored += 1,
            Directed::Respond => {
                eprintln!("Ambient line unexpectedly responded to: {line:?}");
            }
        }
    }

    let rate = ignored as f64 / AMBIENT_CORPUS.len() as f64;
    println!(
        "measured ambient ignored rate: {} / {} ({:.1}%)",
        ignored,
        AMBIENT_CORPUS.len(),
        rate * 100.0
    );
    assert!(
        rate >= 0.90,
        "Ambient ignored rate {rate:.2} fell below 0.90 threshold ({ignored}/{})",
        AMBIENT_CORPUS.len()
    );
}

#[test]
fn test_filler_ignored_reasons() {
    assert_eq!(
        directed::assess("uh", false, 0),
        Directed::Ignore(IgnoreReason::Filler)
    );
    assert_eq!(
        directed::assess("um", false, 0),
        Directed::Ignore(IgnoreReason::Filler)
    );
    assert_eq!(
        directed::assess("", false, 0),
        Directed::Ignore(IgnoreReason::Filler)
    );
}

#[test]
fn test_monologue_ignored_reason() {
    let long_text = "word ".repeat(45);
    assert_eq!(
        directed::assess(&long_text, false, 0),
        Directed::Ignore(IgnoreReason::Monologue)
    );
}

#[test]
fn her_own_voice_coming_back_through_the_microphone_is_not_the_owner() {
    use octoplus_knowledge::companion::engine::CompanionEngine;
    use octoplus_knowledge::companion::persona::Persona;
    use std::sync::Arc;
    let said = "Why did the coffee file a police report? Because it got mugged.";
    // The whole thing, a clipped start, and a garbled middle all count as her echo.
    for heard in [
        "why did the coffee file a police report because it got mugged",
        "why did the coffee file a police report",
        "did the coffee file a police report because got mugged",
    ] {
        assert!(directed::is_echo_of(heard, said), "{heard}");
    }
    // The owner talking about the same subject, or a short reply, is not an echo.
    for heard in [
        "that joke was terrible",
        "tell me another joke about tea",
        "why is the sky blue",
        "yes",
        "haha good one",
    ] {
        assert!(!directed::is_echo_of(heard, said), "{heard}");
    }
    assert!(!directed::is_echo_of(
        "why did the coffee file a police report",
        ""
    ));

    // And the engine drops it before it can become a turn.
    let mut e = CompanionEngine::new(Arc::new(Persona::default_packaged().unwrap()), 1, 1);
    e.push_opener(said, 2);
    assert_eq!(
        e.directed("why did the coffee file a police report because it got mugged"),
        Directed::Ignore(IgnoreReason::EchoOfHer)
    );
    assert_eq!(
        e.directed("what is the capital of france"),
        Directed::Respond
    );
}
