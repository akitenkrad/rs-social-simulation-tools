use socsim_games::llm::{
    ActionCounts, ActionParser, CountsRenderer, GameRenderer, LabelParser, MarkdownTableRenderer,
    OwnHistory, ParseOutcome, ViolationKind,
};
use socsim_games::{MatrixGame, StepRecord};
use socsim_llm::{parse_integer_value, parse_json_object};

fn labeled_game() -> MatrixGame {
    MatrixGame::bimatrix(
        &[vec![3.0, 0.0], vec![5.0, 1.0]],
        &[vec![3.0, 5.0], vec![0.0, 1.0]],
    )
    .unwrap()
    .with_labels(vec![
        vec!["Bid".to_owned(), "Drop".to_owned()],
        vec!["Bid".to_owned(), "Drop".to_owned()],
    ])
    .unwrap()
}

fn accepted(outcome: ParseOutcome<usize>) -> (usize, socsim_games::llm::ParseFlags) {
    match outcome {
        ParseOutcome::Ok { action, flags } => (action, flags),
        ParseOutcome::Violation(violation) => panic!("unexpected violation: {violation:?}"),
    }
}

fn violation_kind(outcome: ParseOutcome<usize>) -> ViolationKind {
    match outcome {
        ParseOutcome::Ok { action, .. } => panic!("unexpected action: {action}"),
        ParseOutcome::Violation(violation) => violation.kind,
    }
}

#[test]
fn strict_label_parser_accepts_exact_plain_and_fenced_json() {
    let game = labeled_game();
    let parser = LabelParser::strict().require_reason(true);

    let (action, flags) =
        accepted(parser.parse(r#"{"action":"Bid","reason":"continue"}"#, &game, Some(0)));
    assert_eq!(action, 0);
    assert!(!flags.fenced);
    assert!(flags.value_was_string);
    assert!(!flags.lenient_match);

    let (action, flags) = accepted(parser.parse(
        "```JSON\n{\"action\":\"Drop\",\"reason\":\"stop\"}\n```",
        &game,
        Some(0),
    ));
    assert_eq!(action, 1);
    assert!(flags.fenced);
}

#[test]
fn strict_label_parser_rejects_bad_json_missing_reason_wrong_case_and_range() {
    let game = labeled_game();
    let parser = LabelParser::strict().require_reason(true);

    assert_eq!(
        violation_kind(parser.parse("not json", &game, Some(0))),
        ViolationKind::Format
    );
    assert_eq!(
        violation_kind(parser.parse(r#"{"action":"Bid"}"#, &game, Some(0))),
        ViolationKind::Format
    );
    assert_eq!(
        violation_kind(parser.parse(r#"{"action":"Bid","reason":7}"#, &game, Some(0),)),
        ViolationKind::Format
    );
    assert_eq!(
        violation_kind(parser.parse(
            r#"{"action":"bid","reason":"case differs"}"#,
            &game,
            Some(0),
        )),
        ViolationKind::Label
    );
    assert_eq!(
        violation_kind(parser.parse(r#"{"action":"Bid","reason":"bad role"}"#, &game, Some(2),)),
        ViolationKind::Range
    );
}

#[test]
fn dollar_protocol_integer_string_and_domain_range_stay_separate() {
    let raw = "```json\n{\"action\":\"Bid\",\"amount\":\" 003 \",\"reason\":\"continue\"}\n```";
    let parsed = parse_json_object(raw).unwrap();
    assert_eq!(
        parse_integer_value(parsed.object.get("amount")),
        Some((3, true))
    );
    assert_eq!(
        accepted(
            LabelParser::strict()
                .require_reason(true)
                .parse(raw, &labeled_game(), Some(0))
        )
        .0,
        0
    );

    let outside =
        parse_json_object(r#"{"action":"Bid","amount":"21","reason":"continue"}"#).unwrap();
    let (amount, was_string) = parse_integer_value(outside.object.get("amount")).unwrap();
    assert!(was_string);
    assert!(!(11..=20).contains(&amount));
}
#[test]
fn lenient_label_parser_uses_word_boundaries_case_folding_and_synonyms() {
    let game = labeled_game();
    let parser = LabelParser::lenient()
        .with_synonyms("Bid", &["raise", "continue"])
        .with_synonyms("Drop", &["quit"]);

    let (action, flags) = accepted(parser.parse("I would RAISE this round.", &game, Some(0)));
    assert_eq!(action, 0);
    assert!(flags.lenient_match);

    let (action, flags) = accepted(parser.parse("Quit now; do not bid.", &game, Some(0)));
    assert_eq!(action, 1, "the first matched choice must win");
    assert!(flags.lenient_match);

    assert_eq!(
        violation_kind(parser.parse("The bidder continued.", &game, Some(0))),
        ViolationKind::Label,
        "Bid must not match inside bidder and continue must not match continued"
    );

    let (action, flags) = accepted(parser.parse(
        r#"{"reason":"I choose Bid despite omitting the action field"}"#,
        &game,
        Some(0),
    ));
    assert_eq!(action, 0, "any failed exact parse falls back to free text");
    assert!(flags.lenient_match);
}

#[test]
fn lenient_validation_rejects_non_ascii_and_non_token_labels() {
    let non_ascii = MatrixGame::zero_sum(&[vec![1.0, -1.0], vec![-1.0, 1.0]])
        .unwrap()
        .with_labels(vec![
            vec!["協力".to_owned(), "離脱".to_owned()],
            vec!["協力".to_owned(), "離脱".to_owned()],
        ])
        .unwrap();
    assert!(LabelParser::strict().validate(&non_ascii).is_ok());
    assert!(LabelParser::lenient().validate(&non_ascii).is_err());

    let punctuation = MatrixGame::zero_sum(&[vec![1.0]])
        .unwrap()
        .with_labels(vec![vec!["not ok".to_owned()], vec!["not ok".to_owned()]])
        .unwrap();
    assert!(LabelParser::lenient().validate(&punctuation).is_err());
}

#[test]
fn renderers_derive_labels_payoffs_history_and_counts_from_the_game() {
    let game = labeled_game();
    let history = OwnHistory {
        t: 4,
        role: Some(0),
        mine: vec![StepRecord {
            action: Some(0),
            payoff: 3.0,
            n_played: 1,
        }],
        opponents_last: vec![Some(1)],
    };
    let renderer = MarkdownTableRenderer::default();
    let rules = renderer.rules(&game, Some(0));
    assert!(rules.contains("Bid"));
    assert!(rules.contains("Drop"));
    assert!(rules.contains("(3, 3)"));
    assert!(rules.contains("(0, 5)"));
    let turn = renderer.turn(&history, &game);
    assert!(turn.contains("t=4"));
    assert!(turn.contains("Bid"));
    assert!(turn.contains("Drop"));

    let counts = ActionCounts {
        role: Some(0),
        my_last: Some(1),
        counts: vec![2, 3],
    };
    let counts_turn = CountsRenderer::default().turn(&counts, &game);
    assert!(counts_turn.contains("Bid: 2"));
    assert!(counts_turn.contains("Drop: 3"));
    assert!(counts_turn.contains("my last action: Drop"));
    assert!(serde_json::to_value(counts).unwrap().get("t").is_none());
}
