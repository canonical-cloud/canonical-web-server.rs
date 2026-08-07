use maud::{html, Markup, DOCTYPE};

use super::{
    QuoteRecord, CLOUD_OPTIONS, DATA_OPTIONS, FRAMEWORK_OPTIONS, TIMELINE_OPTIONS,
};

pub fn quote_page(email: Option<&str>, quote: Option<&QuoteRecord>) -> Markup {
    layout(
        "Compliance quote",
        html! {
            main {
                header class="hero" {
                    p class="eyebrow" { "Canonical Cloud" }
                    h1 { "Get a compliance quote in less than 5 minutes" }
                    p class="lede" {
                        "Tell us what you need for SOC 2, NIST, HIPAA, ISO 27001, PCI DSS, GDPR, or FedRAMP. "
                        "We will analyze the scope and return a non-binding budget range."
                    }
                    @if let Some(email) = email {
                        p class="muted" { "Signed in as " (email) }
                    }
                }

                section class="card" {
                    h2 { "Your environment" }
                    form method="post" action="/u/quote"
                        hx-post="/u/quote" hx-target="#quote-result" hx-swap="innerHTML" {
                        div class="grid two" {
                            label {
                                span { "Company name" }
                                input name="company_name" maxlength="200" required;
                            }
                            label {
                                span { "Company website (optional)" }
                                input type="url" name="website" maxlength="300"
                                    placeholder="https://example.com";
                            }
                            label {
                                span { "Employees" }
                                input type="number" name="employee_count" min="1" max="1000000" required;
                            }
                            label {
                                span { "Target timeline" }
                                select name="target_timeline" required {
                                    @for (value, label) in TIMELINE_OPTIONS {
                                        option value=(value) { (label) }
                                    }
                                }
                            }
                        }

                        fieldset {
                            legend { "Frameworks" }
                            div class="choices" {
                                @for (value, label) in FRAMEWORK_OPTIONS {
                                    label class="choice" {
                                        input type="checkbox" name={ "framework_" (value) } value="yes";
                                        span { (label) }
                                    }
                                }
                            }
                        }

                        fieldset {
                            legend { "Cloud and hosting" }
                            div class="choices" {
                                @for (value, label) in CLOUD_OPTIONS {
                                    label class="choice" {
                                        input type="checkbox" name={ "cloud_" (value) } value="yes";
                                        span { (label) }
                                    }
                                }
                            }
                        }

                        fieldset {
                            legend { "Sensitive data" }
                            div class="choices" {
                                @for (value, label) in DATA_OPTIONS {
                                    label class="choice" {
                                        input type="checkbox" name={ "data_" (value) } value="yes";
                                        span { (label) }
                                    }
                                }
                            }
                        }

                        label {
                            span { "Controls already in place" }
                            textarea name="current_controls" rows="5" maxlength="4000" required
                                placeholder="For example: SSO, MFA, endpoint management, logging, incident response…" {}
                        }
                        label {
                            span { "Anything else we should know? (optional)" }
                            textarea name="notes" rows="4" maxlength="4000"
                                placeholder="Do not include passwords, API keys, patient data, card numbers, or other secrets." {}
                        }
                        p class="muted small" {
                            "The estimate is informational, not legal advice or a certification guarantee. A specialist confirms scope before work begins."
                        }
                        button type="submit" { "Analyze my requirements" }
                    }
                }

                div id="quote-result" aria-live="polite" {
                    @if let Some(quote) = quote {
                        (quote_status(quote))
                    }
                }
            }
        },
    )
}

pub fn quote_status(quote: &QuoteRecord) -> Markup {
    match quote.status.as_str() {
        "queued" | "analyzing" => pending_status(quote),
        "ready" => ready_status(quote),
        "failed" => failed_status(quote),
        _ => html! {
            section id="quote-status" class="card" {
                h2 { "Quote status" }
                p class="muted" { (quote.status) }
            }
        },
    }
}

fn pending_status(quote: &QuoteRecord) -> Markup {
    let url = format!("/u/quote/{}/status", quote.id);
    html! {
        section id="quote-status" class="card" hx-get=(url)
            hx-trigger="load delay:1s, every 2s" hx-swap="outerHTML" {
            h2 { "Analyzing your requirements" }
            p { "Status: " strong { (quote.status) } }
            p class="muted" {
                "Your request is saved. You can leave this page and return with the same signed-in account."
            }
        }
    }
}

fn ready_status(quote: &QuoteRecord) -> Markup {
    html! {
        section id="quote-status" class="card success" {
            h2 { "Indicative quote ready" }
            @if let Some(estimate) = &quote.estimate {
                p class="estimate" {
                    (format_money(estimate.low_cents, &estimate.currency))
                    " – "
                    (format_money(estimate.high_cents, &estimate.currency))
                }
                p class="muted" { "Non-binding implementation budget range." }
            }
            @if let Some(analysis) = &quote.analysis {
                h3 { "Scope summary" }
                p { (analysis.executive_summary) }
                (string_list("Recommended scope", &analysis.recommended_scope))
                (string_list("Key risks", &analysis.risks))
                (string_list("Assumptions", &analysis.assumptions))
                (string_list("Follow-up questions", &analysis.follow_up_questions))
                p class="muted" { "Complexity: " (analysis.complexity.replace('_', " ")) }
            }
            p class="muted small" {
                "Quote ID: " code { (quote.id) }
                @if let Some(version) = quote.context_version {
                    " · context version " (version)
                }
            }
        }
    }
}

fn failed_status(quote: &QuoteRecord) -> Markup {
    html! {
        section id="quote-status" class="card error-card" {
            h2 { "Analysis could not be completed" }
            p { "Your request remains saved. Please try again or contact Canonical Cloud with the quote ID below." }
            p class="muted small" {
                "Quote ID: " code { (quote.id) }
                @if let Some(code) = &quote.failure_code {
                    " · " (code)
                }
            }
        }
    }
}

fn string_list(title: &str, values: &[String]) -> Markup {
    if values.is_empty() {
        return html! {};
    }
    html! {
        h3 { (title) }
        ul {
            @for value in values {
                li { (value) }
            }
        }
    }
}

fn format_money(cents: i64, currency: &str) -> String {
    let whole = cents.saturating_div(100);
    let formatted = group_thousands(whole);
    if currency == "USD" {
        format!("${formatted}")
    } else {
        format!("{formatted} {currency}")
    }
}

fn group_thousands(value: i64) -> String {
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(character);
    }
    grouped
}

fn layout(title: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="color-scheme" content="light dark";
                title { (title) " · canonical.plus" }
                style {
                    "body{font-family:ui-sans-serif,system-ui,sans-serif;max-width:74rem;margin:0 auto;padding:2rem;line-height:1.55}nav{display:flex;justify-content:space-between;align-items:center}.hero{max-width:54rem;margin:4rem 0 2rem}.eyebrow{text-transform:uppercase;letter-spacing:.12em;font-weight:700}.lede{font-size:1.2rem}.card{border:1px solid #8886;border-radius:1rem;padding:1.5rem;margin:1.5rem 0}.grid{display:grid;gap:1rem}.grid.two{grid-template-columns:repeat(auto-fit,minmax(15rem,1fr))}label>span,legend{display:block;font-weight:650;margin-bottom:.35rem}label{display:block;margin:.75rem 0}input,textarea,select,button{font:inherit;padding:.7rem}input,textarea,select{box-sizing:border-box;width:100%}fieldset{border:0;padding:0;margin:1.5rem 0}.choices{display:grid;grid-template-columns:repeat(auto-fit,minmax(13rem,1fr));gap:.5rem}.choice{display:flex;gap:.55rem;align-items:center;border:1px solid #8885;border-radius:.6rem;padding:.65rem}.choice input{width:auto;margin:0}button{cursor:pointer;font-weight:700}.muted{opacity:.72}.small{font-size:.9rem}.estimate{font-size:2rem;font-weight:800}.success{border-color:#19875488}.error-card{border-color:#b4231888}code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace}"
                }
                script type="module" src="/app-assets/app.js" {}
            }
            body {
                nav {
                    a href="https://canonical.plus" { strong { "canonical.plus" } }
                    a href="/u/quote" { "Get a quote" }
                }
                (body)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_is_rendered_without_floating_point() {
        assert_eq!(format_money(1_525_000, "USD"), "$15,250");
        assert_eq!(format_money(99, "USD"), "$0");
    }
}
