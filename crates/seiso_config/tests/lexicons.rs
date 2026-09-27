use seiso_config::Config;

#[test]
fn every_lexicon_extension_accepts_nonempty_phrases_and_rejects_blanks() {
    let root = std::env::current_dir().unwrap();
    for field in [
        "extend-stale-markers",
        "extend-constraint-markers",
        "extend-commit-contexts",
        "extend-pointer-markers",
        "extend-source-pointers",
        "extend-rationale-headings",
        "extend-conversation-markers",
    ] {
        let valid = format!("[lint.lexicon.en]\n{field} = ['custom phrase']");
        assert!(Config::parse(&valid, &root).is_ok(), "{field}");
        let invalid = format!("[lint.lexicon.en]\n{field} = ['  ']");
        let error = Config::parse(&invalid, &root).unwrap_err().to_string();
        assert!(error.contains(field), "{error}");
        assert!(error.contains("must not be empty"), "{error}");
    }
}
