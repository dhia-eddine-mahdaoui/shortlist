//! Small text helpers: salary parsing and the short job brief handed to the AI
//! (requirements kept, benefits / company boilerplate / legal text dropped), to save tokens.

/// Section headings whose content the AI does not need for judging fit.
const DROP_HEADINGS: &[&str] = &[
    "benefit", "perk", "what we offer", "we offer", "why join", "why you'll love", "why work", "why us",
    "about us", "about the company", "who we are", "equal opportunit", "diversity", "inclusion", "eeo",
    "privacy", "our values", "culture", "compensation", "salary", "package", "how to apply",
    "application process", "interview process", "recruitment process", "hiring process", "life at",
    "what's in it for you", "rewards", "in return", "location",
];

/// Lines that are boilerplate wherever they appear.
const DROP_LINES: &[&str] = &[
    "equal opportunity", "regardless of race", "reasonable accommodation", "without regard to",
    "protected characteristic", "we are committed to", "we celebrate", "recruitment agencies",
    "do not accept unsolicited", "privacy notice", "privacy policy",
];

fn is_heading(line: &str) -> Option<&str> {
    if let Some(h) = line.strip_prefix("## ") {
        return Some(h);
    }
    if line.chars().count() < 70 && line.ends_with(':') && !line.starts_with("- ") {
        return Some(line);
    }
    None
}

/// What the employer asks for comes first, then what the job involves, then the rest.
fn section_rank(heading: &str) -> u8 {
    let h = heading.to_lowercase();
    const ASKS: &[&str] = &["require", "qualif", "looking for", "you have", "you'll have", "you bring", "about you", "who you are",
                            "skills", "experience", "must", "nice to have", "bonus", "ideal", "essential", "desirable", "need"];
    const DOES: &[&str] = &["responsib", "what you'll do", "what you will do", "you will", "the role", "role", "day to day", "your impact", "duties", "mission", "build"];
    if ASKS.iter().any(|k| h.contains(k)) {
        0
    } else if DOES.iter().any(|k| h.contains(k)) {
        1
    } else if h.is_empty() {
        2
    } else {
        3
    }
}

/// The job description trimmed to what matters for fit, at most `cap` characters.
pub fn brief(desc: &str, cap: usize) -> String {
    // Split into (heading, lines) sections, dropping boilerplate sections and lines.
    let mut sections: Vec<(String, Vec<String>)> = vec![(String::new(), Vec::new())];
    let mut dropping = false;
    for raw in desc.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(h) = is_heading(line) {
            let lower = h.to_lowercase();
            dropping = DROP_HEADINGS.iter().any(|d| lower.contains(d));
            if !dropping {
                let h = h.trim_end_matches(':').trim().to_string();
                // "Responsibilities:" directly followed by "Software development:" -> keep the outer name.
                match sections.last_mut() {
                    Some((prev, body)) if body.is_empty() && !prev.is_empty() => *prev = format!("{prev} / {h}"),
                    _ => sections.push((h, Vec::new())),
                }
            }
            continue;
        }
        if dropping {
            continue;
        }
        let lower = line.to_lowercase();
        if DROP_LINES.iter().any(|d| lower.contains(d)) {
            continue;
        }
        let line = if line.chars().count() > 260 { format!("{}…", line.chars().take(259).collect::<String>()) } else { line.to_string() };
        sections.last_mut().unwrap().1.push(line);
    }
    sections.retain(|(_, body)| !body.is_empty());
    sections.sort_by_key(|(h, _)| section_rank(h));
    let mut lines: Vec<String> = Vec::new();
    for (h, body) in sections {
        if !h.is_empty() {
            lines.push(format!("# {h}"));
        }
        lines.extend(body);
    }
    let mut out = String::new();
    for l in lines {
        if out.chars().count() + l.chars().count() + 1 > cap {
            let room = cap.saturating_sub(out.chars().count());
            if room > 80 {
                out.push_str(&l.chars().take(room - 1).collect::<String>());
            }
            out.push('…');
            return out;
        }
        out.push_str(&l);
        out.push('\n');
    }
    out.trim_end().to_string()
}

pub fn guess_workplace(location: &str, title: &str, desc: &str) -> String {
    let hay = format!("{location} {title}").to_lowercase();
    if hay.contains("remote") {
        return "Remote".into();
    }
    if hay.contains("hybrid") {
        return "Hybrid".into();
    }
    let d = desc.to_lowercase();
    let head: String = d.chars().take(600).collect();
    if head.contains("fully remote") || head.contains("100% remote") || head.contains("(remote)") {
        return "Remote".into();
    }
    if head.contains("hybrid") {
        return "Hybrid".into();
    }
    if head.contains("on-site") || head.contains("onsite") || head.contains("(on site)") {
        return "On-site".into();
    }
    String::new()
}

fn currency_of(s: &str) -> String {
    let l = s.to_lowercase();
    let c = if s.contains('£') || l.contains("gbp") {
        "GBP"
    } else if s.contains('€') || l.contains("eur") {
        "EUR"
    } else if s.contains('$') || l.contains("usd") {
        if l.contains("cad") || l.contains("ca$") { "CAD" } else if l.contains("aud") || l.contains("a$") { "AUD" } else { "USD" }
    } else if s.contains('₺') || l.contains("try") {
        "TRY"
    } else if l.contains("chf") {
        "CHF"
    } else if l.contains("pln") {
        "PLN"
    } else {
        ""
    };
    c.to_string()
}

/// Finds a salary written in the description ("£70,000 - £100,000 + equity").
pub fn salary_in_text(desc: &str) -> Option<String> {
    let chars: Vec<char> = desc.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if matches!(c, '£' | '$' | '€') && chars.get(i + 1).map_or(false, |n| n.is_ascii_digit()) {
            let end = (i + 60).min(chars.len());
            let rest: String = chars[i..end].iter().collect();
            let first = rest.lines().next().unwrap_or("");
            const WORDS: &[&str] = &["-", "–", "to", "per", "a", "an", "annum", "year", "yr", "hour", "hr", "day", "month", "pa", "p.a.", "gbp", "usd", "eur", "k"];
            let mut kept: Vec<&str> = Vec::new();
            for w in first.split_whitespace() {
                let lw = w.to_lowercase();
                let money = w.chars().any(|c| c.is_ascii_digit() || matches!(c, '£' | '$' | '€'));
                if money || WORDS.contains(&lw.trim_matches(|c: char| c == ',' || c == '.')) {
                    kept.push(w);
                } else {
                    break;
                }
            }
            while kept.last().map_or(false, |w| !w.chars().any(|c| c.is_ascii_digit()) && !matches!(w.to_lowercase().trim_matches(|c: char| c == ',' || c == '.'), "annum" | "year" | "hour" | "day" | "month" | "pa")) {
                kept.pop();
            }
            let window = kept.join(" ").trim_end_matches(|c: char| c == ',' || c == '.').to_string();
            if let Some((lo, _, _)) = parse_salary(&window) {
                if lo >= 10_000.0 {
                    return Some(window);
                }
            }
        }
    }
    None
}

/// "70K GBP/yr - 100K GBP/yr" -> (70000, 100000, "GBP"). Hourly, daily and monthly pay are turned into yearly.
pub fn parse_salary(s: &str) -> Option<(f64, f64, String)> {
    if s.trim().is_empty() {
        return None;
    }
    let chars: Vec<char> = s.chars().collect();
    let mut nums: Vec<f64> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == ',' || chars[i] == '.') {
                i += 1;
            }
            let raw: String = chars[start..i].iter().filter(|c| **c != ',').collect();
            let raw = raw.trim_end_matches('.');
            if let Ok(mut n) = raw.parse::<f64>() {
                match chars.get(i) {
                    Some('k') | Some('K') => n *= 1000.0,
                    Some('m') | Some('M') if !matches!(chars.get(i + 1), Some('o') | Some('O')) => n *= 1_000_000.0,
                    _ => {}
                }
                if n > 0.0 {
                    nums.push(n);
                }
            }
        } else {
            i += 1;
        }
    }
    if nums.is_empty() {
        return None;
    }
    let l = s.to_lowercase();
    let mult = if l.contains("/hr") || l.contains("hour") {
        2080.0
    } else if l.contains("/day") || l.contains("per day") || l.contains("daily") || l.contains("a day") {
        230.0
    } else if l.contains("/mo") || l.contains("month") {
        12.0
    } else {
        1.0
    };
    let lo = nums[0] * mult;
    let hi = nums.get(1).copied().unwrap_or(nums[0]) * mult;
    let (lo, hi) = if hi < lo { (hi, lo) } else { (lo, hi) };
    if hi < 1000.0 {
        return None;
    }
    Some((lo, hi, currency_of(s)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn salaries() {
        assert_eq!(parse_salary("70K GBP/yr - 100K GBP/yr"), Some((70000.0, 100000.0, "GBP".into())));
        assert_eq!(parse_salary("£70,000 - £100,000 + Equity"), Some((70000.0, 100000.0, "GBP".into())));
        assert_eq!(parse_salary("$50/hr - $60/hr"), Some((104000.0, 124800.0, "USD".into())));
        assert_eq!(parse_salary("Up to £70k"), Some((70000.0, 70000.0, "GBP".into())));
        assert_eq!(parse_salary("5 days ago"), None);
        assert_eq!(salary_in_text("London\n£70,000 - £100,000 + Equity + Annual Salary Reviews").as_deref(), Some("£70,000 - £100,000"));
        assert_eq!(salary_in_text("Pay: $120k to $150k per year, plus bonus").as_deref(), Some("$120k to $150k per year"));
    }

    #[test]
    fn brief_drops_boilerplate() {
        let d = "## About us\nWe are great.\n## What you'll do\n- Build a grading tool for schools\n## Benefits\n- Free lunch\nWe are an equal opportunity employer.";
        let b = brief(d, 2000);
        assert!(b.contains("grading tool"));
        assert!(!b.contains("Free lunch"));
        assert!(!b.contains("We are great"));
        assert!(!b.contains("equal opportunity"));
    }

    #[test]
    fn brief_puts_requirements_first() {
        let d = "We build things.\n## Responsibilities:\n## Development:\n- Write code\n## Requirements:\n- 5 years of Rust";
        let b = brief(d, 2000);
        assert!(b.starts_with("# Requirements\n- 5 years of Rust"), "{b}");
        assert!(b.contains("# Responsibilities / Development\n- Write code"), "{b}");
    }
}
