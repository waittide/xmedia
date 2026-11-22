use super::*;

struct MockJsEvaluator;

impl JsRuleEvaluator for MockJsEvaluator {
    fn eval_rule_js(
        &mut self,
        code: &str,
        input: &RuleValue,
        base_url: &str,
    ) -> Result<RuleValue, String> {
        let trimmed = code.trim();
        if trimmed == "upper" {
            return Ok(RuleValue::Text(input.as_text().to_ascii_uppercase()));
        }
        if trimmed == "wrap_json" {
            return Ok(RuleValue::Json(serde_json::json!({
                "val": input.as_text(),
                "base": base_url
            })));
        }
        if trimmed == "make_list" {
            return Ok(RuleValue::Json(serde_json::json!(["alpha", "beta"])));
        }
        Err(format!("unknown mock op: {trimmed}"))
    }
}

#[test]
fn html_data_url_resolves_relative_and_preserves_absolute() {
    let html = r#"
        <div id="_imageList">
            <img data-url="/img/p1.jpg"/>
            <img data-url="https://cdn.example.com/p2.jpg"/>
            <img data-url="data:image/png;base64,AAA"/>
        </div>
        <div data-id="12345"></div>
    "#;
    let base_url = "https://comic.example.com/viewer/1";
    let rv = RuleValue::Text(html.to_string());

    let joined = eval_rule_string(&rv, "#_imageList img@data-url", base_url, None).unwrap();
    assert_eq!(
        joined,
        "https://comic.example.com/img/p1.jpg\nhttps://cdn.example.com/p2.jpg\ndata:image/png;base64,AAA"
    );

    let list = eval_rule_list(&rv, "#_imageList img", base_url, None).unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(
        eval_rule_string(&list[0], "data-url", base_url, None).unwrap(),
        "https://comic.example.com/img/p1.jpg"
    );
    assert_eq!(
        eval_rule_string(&list[0], "@DATA-URL", base_url, None).unwrap(),
        "https://comic.example.com/img/p1.jpg"
    );

    // Non-URL data-* attribute must not be joined against base_url
    assert_eq!(
        eval_rule_string(&rv, "div[data-id]@data-id", base_url, None).unwrap(),
        "12345"
    );

    // Empty base_url keeps relative data-url unchanged
    assert_eq!(
        eval_rule_string(&list[0], "data-url", "", None).unwrap(),
        "/img/p1.jpg"
    );
}

#[test]
fn jsonpath_supports_fields_wildcards_slices_negative_indices_and_recursive_descent() {
    let doc = serde_json::json!({
        "data": {
            "items": [
                {"id": 10, "name": "One"},
                {"id": 20, "name": "Two"},
                {"id": 30, "name": "Three"}
            ]
        }
    });
    let rv = RuleValue::Json(doc);
    assert_eq!(
        eval_rule_string(&rv, "$.data.items[*].name", "", None).unwrap(),
        "One\nTwo\nThree"
    );
    assert_eq!(
        eval_rule_string(&rv, "$.data.items[-1].name", "", None).unwrap(),
        "Three"
    );
    let sliced = eval_rule_list(&rv, "$.data.items[0:2]", "", None).unwrap();
    assert_eq!(sliced.len(), 2);
    assert_eq!(
        eval_rule_string(&rv, "@Json:$..id", "", None).unwrap(),
        "10\n20\n30"
    );

    // Depth cap safety on deeply nested JSON (>128 levels)
    let mut deep = serde_json::json!({"target": "deep-val"});
    for _ in 0..160 {
        deep = serde_json::json!({"nest": deep});
    }
    let matches = eval_jsonpath(&deep, "$..target");
    assert!(matches.is_empty());
}

#[test]
fn html_css_and_legado_shorthand_chaining() {
    let html = r#"
        <div id="list">
            <div class="item">
                <a href="/a1">First</a>
                <a href="/a2">LastLink</a>
            </div>
            <div class="item">
                <p>Line1<br/><span>Inner</span>Line2</p>
            </div>
        </div>
    "#;
    let rv = RuleValue::Text(html.to_string());
    assert_eq!(
        eval_rule_string(
            &rv,
            "class.item.0@tag.a.-1@href",
            "https://example.com/root",
            None
        )
        .unwrap(),
        "https://example.com/a2"
    );

    let children = eval_rule_list(&rv, "id.list@children[!0]", "", None).unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(
        eval_rule_string(&children[0], "p@ownText", "", None).unwrap(),
        "Line1Line2"
    );
    assert_eq!(
        eval_rule_string(&children[0], "p@textNodes", "", None).unwrap(),
        "Line1\nInner\nLine2"
    );
    assert!(
        eval_rule_string(&children[0], "p@html", "", None)
            .unwrap()
            .contains("<span>Inner</span>")
    );
    assert!(
        eval_rule_string(&children[0], "p@all", "", None)
            .unwrap()
            .starts_with("<p>")
    );

    let sliced = eval_rule_list(&rv, "class.item[0:1]", "", None).unwrap();
    assert_eq!(sliced.len(), 1);
}

#[test]
fn regex_capture_and_double_hash_replacement() {
    let rv = RuleValue::Text(
        r#"<div class="title">诡秘之主（精校版）</div> id=98765 num=123-456"#.to_string(),
    );
    assert_eq!(
        eval_rule_string(&rv, r":id=(\d+)", "", None).unwrap(),
        "98765"
    );
    assert_eq!(
        eval_rule_string(&rv, "class.title@text##（精校版）", "", None).unwrap(),
        "诡秘之主"
    );
    // ### replaces first match only
    assert_eq!(
        eval_rule_string(&rv, r":num=([\d-]+)##\d+##X###", "", None).unwrap(),
        "X-456"
    );
    // Without ### replaces all matches
    assert_eq!(
        eval_rule_string(&rv, r":num=([\d-]+)##\d+##X", "", None).unwrap(),
        "X-X"
    );

    let json_rv = RuleValue::Json(serde_json::json!({"a": "foo#1", "b": "bar"}));
    assert_eq!(
        eval_rule_string(&json_rv, "{{$.a}}-{{$.b}}##foo#1##baz", "", None).unwrap(),
        "baz-bar"
    );
}

#[test]
fn mustache_and_combinators() {
    let json_rv = RuleValue::Json(serde_json::json!({
        "a": "hello",
        "b": "world",
        "empty": "",
        "left": ["L1", "L2"],
        "right": ["R1", "R2"]
    }));

    assert_eq!(
        eval_rule_string(&json_rv, "{{$.a}}-{{$.b}}", "", None).unwrap(),
        "hello-world"
    );
    assert_eq!(
        eval_rule_string(&json_rv, "$.empty || $.b", "", None).unwrap(),
        "world"
    );
    assert_eq!(
        eval_rule_string(&json_rv, "$.a && $.b", "", None).unwrap(),
        "hello\nworld"
    );

    let interleaved = eval_rule_list(&json_rv, "$.left %% $.right", "", None).unwrap();
    let texts: Vec<String> = interleaved.into_iter().map(|v| v.as_text()).collect();
    assert_eq!(texts, vec!["L1", "R1", "L2", "R2"]);

    let reversed = eval_rule_list(&json_rv, "-$.left", "", None).unwrap();
    let rev_texts: Vec<String> = reversed.into_iter().map(|v| v.as_text()).collect();
    assert_eq!(rev_texts, vec!["L2", "L1"]);

    assert!(rule_needs_js("<js>1+1</js>"));
    assert!(rule_needs_js("{{key}}"));
    assert!(!rule_needs_js("{{$.a}}-{{$.b}}"));
    assert!(eval_rule_string(&json_rv, "@js:result", "", None).is_err());
}

#[test]
fn js_rule_evaluator_trait_pipeline() {
    let html = RuleValue::Text(r#"<span class="name">hello</span>"#.to_string());
    let mut mock = MockJsEvaluator;

    let out = eval_rule_string(
        &html,
        "class.name@text<js>upper</js>",
        "https://example.com",
        Some(&mut mock),
    )
    .unwrap();
    assert_eq!(out, "HELLO");

    let chained = eval_rule_string(
        &html,
        "class.name@text<js>wrap_json</js>$.val",
        "https://example.com",
        Some(&mut mock),
    )
    .unwrap();
    assert_eq!(chained, "hello");

    let list_out = eval_rule_list(
        &html,
        "@js:make_list",
        "https://example.com",
        Some(&mut mock),
    )
    .unwrap();
    assert_eq!(
        list_out
            .into_iter()
            .map(|v| v.as_text())
            .collect::<Vec<_>>(),
        vec!["alpha", "beta"]
    );
}

#[test]
fn css_attribute_selectors_with_colons_are_not_stripped_as_slice_filters() {
    let html = RuleValue::Text(
        r#"
        <div>
            <img data-url="http://bad.example/1.jpg"/>
            <img data-url="https://good.example/2.jpg"/>
            <img data-url="https://good.example/3.jpg"/>
            <a href="/relative">Rel</a>
            <a href="https://example.com/abs">Abs</a>
        </div>
        "#
        .into(),
    );

    assert_eq!(
        eval_rule_string(&html, r#"img[data-url^="https://"]@data-url"#, "", None).unwrap(),
        "https://good.example/2.jpg\nhttps://good.example/3.jpg"
    );
    assert_eq!(
        eval_rule_string(&html, r#"a[href*="://"]@href"#, "", None).unwrap(),
        "https://example.com/abs"
    );
    assert_eq!(
        eval_rule_string(&html, r#"img[data-url^="https://"][0]@data-url"#, "", None).unwrap(),
        "https://good.example/2.jpg"
    );
    // Exclusion with comma-separated indices still works
    assert_eq!(
        eval_rule_string(&html, r#"img[!0,2]@data-url"#, "", None).unwrap(),
        "https://good.example/2.jpg"
    );
}

#[test]
fn jsonpath_supports_filter_predicates() {
    let doc = serde_json::json!({
        "relationships": [
            { "type": "cover_art", "attributes": { "name": "Cover" }, "active": false },
            { "type": "author", "attributes": { "name": "Kishimoto Mock" }, "active": true },
            { "type": "artist", "attributes": { "name": "Artist Mock" }, "count": 2 }
        ]
    });
    let rv = RuleValue::Json(doc);

    assert_eq!(
        eval_rule_string(
            &rv,
            "$.relationships[?(@.type=='author')].attributes.name",
            "",
            None
        )
        .unwrap(),
        "Kishimoto Mock"
    );
    assert_eq!(
        eval_rule_string(
            &rv,
            r#"$.relationships[?(@.type!="cover_art")].type"#,
            "",
            None
        )
        .unwrap(),
        "author\nartist"
    );
    assert_eq!(
        eval_rule_string(&rv, "$.relationships[?(@.active)].type", "", None).unwrap(),
        "author"
    );
    assert_eq!(
        eval_rule_string(&rv, "$.relationships[?(!@.active)].type", "", None).unwrap(),
        "cover_art\nartist"
    );
    assert_eq!(
        eval_rule_string(
            &rv,
            "$.relationships[?(@.count==2)].attributes.name",
            "",
            None
        )
        .unwrap(),
        "Artist Mock"
    );
}

#[test]
fn jsonpath_filter_handles_utf8_fields_escaped_quotes_and_double_hash() {
    let doc = serde_json::json!({
        "items": [
            { "标题": "命中", "v": "1" },
            { "标题": "未命中", "v": "2" },
            { "name": "O'Reilly", "title": "SingleQuote" },
            { "name": "a\"b", "title": "DoubleQuote" },
            { "name": "back\\slash\nline\t", "title": "Escapes" },
            { "name": "a##b", "title": "HashMatched" }
        ]
    });
    let rv = RuleValue::Json(doc);

    // Non-ASCII UTF-8 field name before operator must not panic on char boundary
    assert_eq!(
        eval_rule_string(&rv, "$.items[?(@.标题=='命中')].v", "", None).unwrap(),
        "1"
    );
    assert_eq!(
        eval_rule_string(&rv, "$.items[?(@.标题==='命中')].v", "", None).unwrap(),
        "1"
    );

    // Escaped quotes & backslash sequences inside filter string literals
    assert_eq!(
        eval_rule_string(&rv, r"$.items[?(@.name=='O\'Reilly')].title", "", None).unwrap(),
        "SingleQuote"
    );
    assert_eq!(
        eval_rule_string(&rv, r#"$.items[?(@.name=="a\"b")].title"#, "", None).unwrap(),
        "DoubleQuote"
    );
    assert_eq!(
        eval_rule_string(
            &rv,
            r"$.items[?(@.name=='back\\slash\nline\t')].title",
            "",
            None
        )
        .unwrap(),
        "Escapes"
    );

    // `##` inside JSONPath filter quotes must NOT be treated as regex-replace separator
    assert_eq!(
        eval_rule_string(&rv, "$.items[?(@.name=='a##b')].title", "", None).unwrap(),
        "HashMatched"
    );
    // `##` AFTER the bracketed filter still acts as regex-replace separator
    assert_eq!(
        eval_rule_string(&rv, "$.items[?(@.name=='a##b')].title##Hash##OK", "", None).unwrap(),
        "OKMatched"
    );
}

#[test]
fn rule_value_from_json_or_raw_accepts_scalar_roots_and_rejects_malformed() {
    assert_eq!(
        RuleValue::from_json_or_raw(r#""just-a-string""#.to_string()),
        RuleValue::Json(serde_json::json!("just-a-string"))
    );
    assert_eq!(
        RuleValue::from_json_or_raw("123".to_string()),
        RuleValue::Json(serde_json::json!(123))
    );
    assert_eq!(
        RuleValue::from_json_or_raw("true".to_string()),
        RuleValue::Json(serde_json::json!(true))
    );
    assert_eq!(
        RuleValue::from_json_or_raw("null".to_string()),
        RuleValue::Json(serde_json::Value::Null)
    );
    assert!(
        RuleValue::from_json_or_raw(r#"{ "broken": [1, 2,"#.to_string())
            .as_json()
            .is_none()
    );
    // Scalar JSON root yields empty list for $.items[*] without error
    let scalar_rv = RuleValue::from_json_or_raw(r#""just-a-string""#.to_string());
    assert!(
        eval_rule_list(&scalar_rv, "$.items[*]", "", None)
            .unwrap()
            .is_empty()
    );
}
