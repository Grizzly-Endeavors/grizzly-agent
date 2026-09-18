use super::*;

#[test]
fn merge_keeps_earlier_field_when_later_event_leaves_it_unset() {
    let first = Usage {
        input_tokens: Some(10),
        output_tokens: None,
    };
    let second = Usage {
        input_tokens: None,
        output_tokens: Some(20),
    };

    let merged = first.merge(second);

    assert_eq!(
        merged,
        Usage {
            input_tokens: Some(10),
            output_tokens: Some(20),
        },
        "a field the later event does not report must keep the earlier value"
    );
}

#[test]
fn merge_overwrites_field_when_later_event_reports_it() {
    let first = Usage {
        input_tokens: Some(10),
        output_tokens: Some(15),
    };
    let second = Usage {
        input_tokens: Some(12),
        output_tokens: None,
    };

    let merged = first.merge(second);

    assert_eq!(
        merged,
        Usage {
            input_tokens: Some(12),
            output_tokens: Some(15),
        },
        "a field the later event reports must replace the earlier value"
    );
}
