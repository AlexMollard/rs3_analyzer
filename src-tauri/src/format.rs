// Reference implementation of the UI's formatGp (ui/lib/format.ts). The app no longer formats
// numbers in Rust; this exists so both sides are checked against the same case table.

pub(crate) fn format_gp(value: f64) -> String {
    if value < 0.0 {
        let s = format_gp(-value);
        return if s == "0" { s } else { format!("-{}", s) };
    }
    if value >= 1_000_000_000.0 {
        format!("{:.2}B", value / 1_000_000_000.0)
    } else if value >= 1_000_000.0 {
        format!("{:.2}M", value / 1_000_000.0)
    } else if value >= 1_000.0 {
        format!("{:.1}K", value / 1_000.0)
    } else {
        format!("{:.0}", value)
    }
}

#[test]
fn format_gp_matches_shared_cases() {
    let cases: Vec<(f64, String)> =
        serde_json::from_str(include_str!("../../ui/lib/format-gp-cases.json")).unwrap();
    for (value, expected) in cases {
        assert_eq!(format_gp(value), expected, "format_gp({})", value);
    }
}
