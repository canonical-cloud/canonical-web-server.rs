use crate::auth::AuthContext;
use crate::db::entity::{audit_engagement, engagement_note};
use maud::{html, Markup, DOCTYPE};
use serde_json::Value;

fn layout(title: &str, body: Markup, csrf: Option<&str>, account_key: Option<&str>) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="color-scheme" content="light dark";
                @if let Some(csrf) = csrf {
                    meta name="csrf-token" content=(csrf);
                }
                @if let Some(account_key) = account_key {
                    meta name="canonical-account-key" content=(account_key);
                }
                title { (title) " · canonical.cloud" }
                style {
                    "body{font-family:ui-sans-serif,system-ui,sans-serif;max-width:72rem;margin:0 auto;padding:2rem;line-height:1.5}nav{display:flex;justify-content:space-between;align-items:center}main{margin-top:3rem}.card{border:1px solid #8886;border-radius:.75rem;padding:1.25rem;margin:1rem 0}label{display:block;margin:.75rem 0}input,textarea,select,button{font:inherit;padding:.65rem}input,textarea,select{box-sizing:border-box;width:100%}button{cursor:pointer}.framework-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(12rem,1fr));gap:.5rem}.framework-grid label{display:flex;gap:.5rem;align-items:center;margin:0}.framework-grid input{width:auto}.quote-total{font-size:1.5rem;font-weight:700}.muted{opacity:.7}.error{color:#b42318}#sync-status[data-state=offline]{color:#b54708}#sync-status[data-state=synced]{color:#067647}"
                }
                script type="module" src="/app-assets/app.js" {}
            }
            body {
                nav {
                    a href="/" { strong { "canonical.cloud" } }
                    span {
                        a href="/app" { "Application" }
                        " · "
                        a href="/app/engagements" { "Engagements" }
                        " · "
                        a href="/u/quote" { "Get a quote" }
                    }
                }
                (body)
            }
        }
    }
}

pub fn login(csrf: &str, error: Option<&str>) -> Markup {
    layout(
        "Sign in",
        html! {
            main {
                h1 { "Sign in" }
                p class="muted" { "Authentication is handled by Supabase; credentials are never stored by this server." }
                @if let Some(error) = error {
                    p class="error" role="alert" { (error) }
                }
                form method="post" action="/auth/login" hx-post="/auth/login" hx-target="#login-result" hx-swap="innerHTML" {
                    input type="hidden" name="csrf" value=(csrf);
                    label { "Email" input type="email" name="email" autocomplete="email" required; }
                    label { "Password" input type="password" name="password" autocomplete="current-password" required; }
                    button type="submit" { "Sign in" }
                }
                div id="login-result" aria-live="polite" {}
            }
        },
        None,
        None,
    )
}

pub fn login_error(message: &str) -> Markup {
    html! {
        p class="error" role="alert" { (message) }
    }
}

pub fn dashboard(actor: &AuthContext) -> Markup {
    let account_key = actor.user_id.to_string();
    layout(
        "Application",
        html! {
            main data-sync-root="draft_note" hx-ext="ws" ws-connect="/ws" {
                header {
                    h1 { "Application" }
                    p { "Signed in as " (actor.email) }
                    p id="sync-status" class="muted" data-state="starting" aria-live="polite" { "Starting offline sync…" }
                    form method="post" action="/auth/logout" {
                        input type="hidden" name="csrf" value=(actor.csrf_token.as_deref().unwrap_or_default());
                        button type="submit" { "Sign out" }
                    }
                }
                section class="card" {
                    h2 { "Optimistic draft note" }
                    p class="muted" { "Edits are committed to IndexedDB first, then reconciled through the REST API. WebSockets only wake the pull loop." }
                    form data-sync-form="draft_note" {
                        input type="hidden" name="id";
                        label { "Title" input name="title" required; }
                        label { "Body" textarea name="body" rows="8" {} }
                        button type="submit" { "Save locally" }
                    }
                    div data-sync-list="draft_note" {}
                    div data-sync-conflicts="draft_note" {}
                }
                section class="card" hx-get="/app/fragments/session" hx-trigger="load" hx-swap="innerHTML" {
                    p class="muted" { "Loading server session fragment…" }
                }
            }
        },
        actor.csrf_token.as_deref(),
        Some(&account_key),
    )
}

pub fn session_fragment(actor: &AuthContext) -> Markup {
    html! {
        div id="session-fragment" {
            strong { "HTMX connected" }
            p class="muted" { "Maud rendered this fragment for user " (actor.user_id) "." }
        }
    }
}

pub fn engagements_page(actor: &AuthContext, engagements: &[audit_engagement::Model]) -> Markup {
    let csrf = actor.csrf_token.as_deref().unwrap_or_default();
    layout(
        "Engagements",
        html! {
            main {
                header {
                    h1 { "Compliance engagements" }
                    p class="muted" { "Engagements are private to your account and enforced by row-level security." }
                }
                section class="card" {
                    h2 { "Open an engagement" }
                    form method="post" action="/app/engagements"
                        hx-post="/app/engagements" hx-target="#engagement-list" hx-swap="outerHTML" {
                        input type="hidden" name="csrf" value=(csrf);
                        label { "Company" input name="company" required maxlength="200"; }
                        label { "Framework"
                            select name="framework" {
                                @for framework in audit_engagement::FRAMEWORKS {
                                    option value=(framework) { (framework_label(framework)) }
                                }
                            }
                        }
                        label { "Target report date (optional)" input type="date" name="target_report_date"; }
                        button type="submit" { "Open engagement" }
                    }
                    div id="engagement-form-error" aria-live="polite" {}
                }
                (engagement_list(engagements))
            }
        },
        actor.csrf_token.as_deref(),
        None,
    )
}

pub fn engagement_list(engagements: &[audit_engagement::Model]) -> Markup {
    html! {
        section id="engagement-list" class="card" {
            h2 { "Your engagements" }
            @if engagements.is_empty() {
                p class="muted" { "No engagements yet. Open one above to start tracking an audit." }
            } @else {
                ul {
                    @for engagement in engagements {
                        li {
                            a href={ "/app/engagements/" (engagement.id) } { (engagement.company) }
                            " — " (framework_label(&engagement.framework))
                            " · " (status_label(&engagement.status))
                            " · opened " (engagement.opened_at.format("%Y-%m-%d"))
                            @if let Some(target) = engagement.target_report_date {
                                " · report due " (target)
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn engagement_form_error(message: &str) -> Markup {
    html! {
        p class="error" role="alert" { (message) }
    }
}

pub fn engagement_detail_page(
    actor: &AuthContext,
    engagement: &audit_engagement::Model,
    notes: &[engagement_note::Model],
) -> Markup {
    let csrf = actor.csrf_token.as_deref().unwrap_or_default();
    layout(
        &engagement.company,
        html! {
            main {
                header {
                    p { a href="/app/engagements" { "← All engagements" } }
                    h1 { (engagement.company) }
                    p class="muted" {
                        (framework_label(&engagement.framework))
                        " · opened " (engagement.opened_at.format("%Y-%m-%d"))
                        @if let Some(target) = engagement.target_report_date {
                            " · report due " (target)
                        }
                    }
                }
                (engagement_status(engagement, csrf))
                section class="card" {
                    h2 { "Notes" }
                    form method="post" action={ "/app/engagements/" (engagement.id) "/notes" }
                        hx-post={ "/app/engagements/" (engagement.id) "/notes" }
                        hx-target="#note-list" hx-swap="outerHTML" {
                        input type="hidden" name="csrf" value=(csrf);
                        label { "Add a note" textarea name="body" rows="4" required maxlength="4000" {} }
                        button type="submit" { "Add note" }
                    }
                    div id="note-form-error" aria-live="polite" {}
                    (engagement_notes(notes))
                }
            }
        },
        actor.csrf_token.as_deref(),
        None,
    )
}

pub fn engagement_status(engagement: &audit_engagement::Model, csrf: &str) -> Markup {
    html! {
        section id="engagement-status" class="card" {
            h2 { "Status: " (status_label(&engagement.status)) }
            form method="post" action={ "/app/engagements/" (engagement.id) "/status" }
                hx-post={ "/app/engagements/" (engagement.id) "/status" }
                hx-target="#engagement-status" hx-swap="outerHTML" {
                input type="hidden" name="csrf" value=(csrf);
                label { "Move to"
                    select name="status" {
                        @for status in audit_engagement::STATUSES {
                            option value=(status) selected[*status == engagement.status] {
                                (status_label(status))
                            }
                        }
                    }
                }
                button type="submit" { "Update status" }
            }
            p class="muted" { "Last updated " (engagement.updated_at.format("%Y-%m-%d %H:%M UTC")) }
        }
    }
}

pub fn engagement_notes(notes: &[engagement_note::Model]) -> Markup {
    html! {
        div id="note-list" {
            @if notes.is_empty() {
                p class="muted" { "No notes yet." }
            } @else {
                ul {
                    @for note in notes {
                        li {
                            p { (note.body) }
                            p class="muted" { (note.created_at.format("%Y-%m-%d %H:%M UTC")) }
                        }
                    }
                }
            }
        }
    }
}

fn framework_label(framework: &str) -> &'static str {
    match framework {
        "soc2" => "SOC 2",
        "fedramp" => "FedRAMP",
        "hipaa" => "HIPAA",
        "iso_27001" => "ISO 27001",
        "pci_dss" => "PCI DSS",
        "gdpr" => "GDPR",
        _ => "Unknown framework",
    }
}

fn status_label(status: &str) -> &'static str {
    match status {
        "scoping" => "Scoping",
        "remediation" => "Remediation",
        "in_audit" => "In audit",
        "complete" => "Complete",
        _ => "Unknown status",
    }
}

pub fn quote_page(email: &str) -> Markup {
    layout(
        "Get a quote",
        html! {
            main {
                header {
                    p { a href="/" { "← canonical.plus" } }
                    h1 { "Get a compliance quote in less than 5 minutes" }
                    p class="muted" {
                        "Signed in"
                        @if !email.is_empty() { " as " (email) }
                        ". We use your answers to estimate scope; this is not an audit opinion or certification."
                    }
                }
                form class="card" method="post" action="/u/quote"
                    hx-post="/u/quote" hx-target="#quote-result" hx-swap="innerHTML"
                    hx-indicator="#quote-progress" {
                    h2 { "Company" }
                    label { "Company name" input name="company_name" required maxlength="200"; }
                    label { "Company size"
                        select name="company_size" required {
                            option value="" { "Choose a range" }
                            option value="1-10" { "1–10 people" }
                            option value="11-50" { "11–50 people" }
                            option value="51-200" { "51–200 people" }
                            option value="201-1000" { "201–1,000 people" }
                            option value="1000+" { "More than 1,000 people" }
                        }
                    }
                    label { "Industry" input name="industry" required maxlength="120"; }

                    h2 { "Frameworks" }
                    p class="muted" { "Choose every framework you want included." }
                    div class="framework-grid" {
                        label { input type="checkbox" name="soc2"; "SOC 2" }
                        label { input type="checkbox" name="nist_csf"; "NIST CSF" }
                        label { input type="checkbox" name="nist_800_53"; "NIST 800-53" }
                        label { input type="checkbox" name="hipaa"; "HIPAA" }
                        label { input type="checkbox" name="iso_27001"; "ISO 27001" }
                        label { input type="checkbox" name="pci_dss"; "PCI DSS" }
                        label { input type="checkbox" name="fedramp"; "FedRAMP" }
                    }

                    h2 { "Scope" }
                    label { "Current compliance stage"
                        select name="current_stage" required {
                            option value="" { "Choose a stage" }
                            option value="Starting from scratch" { "Starting from scratch" }
                            option value="Some controls documented" { "Some controls documented" }
                            option value="Preparing for an audit" { "Preparing for an audit" }
                            option value="Maintaining an existing program" { "Maintaining an existing program" }
                        }
                    }
                    label { "Target timeline"
                        select name="target_timeline" required {
                            option value="" { "Choose a timeline" }
                            option value="Within 30 days" { "Within 30 days" }
                            option value="This quarter" { "This quarter" }
                            option value="Within 6 months" { "Within 6 months" }
                            option value="This year" { "This year" }
                            option value="Still exploring" { "Still exploring" }
                        }
                    }
                    label { "Sensitive data types (comma-separated)"
                        input name="data_types" maxlength="960" placeholder="Customer PII, PHI, payment data";
                    }
                    label { "Cloud and hosting providers (comma-separated)"
                        input name="cloud_providers" maxlength="960" placeholder="AWS, GCP, Azure, Cloudflare";
                    }
                    label { "Anything else we should know"
                        textarea name="notes" rows="5" maxlength="4000" {}
                    }
                    button type="submit" { "Analyze my quote" }
                    span id="quote-progress" class="muted htmx-indicator" { " Analyzing securely…" }
                }
                section id="quote-result" aria-live="polite" {}
            }
        },
        None,
        None,
    )
}

pub fn quote_result(quote: &Value) -> Markup {
    let analysis = quote.get("analysis").unwrap_or(&Value::Null);
    let minimum = analysis
        .get("estimated_min_usd")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let maximum = analysis
        .get("estimated_max_usd")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let weeks = analysis
        .get("estimated_weeks")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let summary = analysis
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or("Your quote is ready.");
    let assumptions = analysis
        .get("assumptions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let next_steps = analysis
        .get("next_steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    html! {
        article class="card" {
            h2 { "Your preliminary quote" }
            p class="quote-total" { "$" (minimum) "–$" (maximum) " USD" }
            p { "Estimated delivery: " (weeks) " weeks" }
            p { (summary) }
            @if !assumptions.is_empty() {
                h3 { "Assumptions" }
                ul {
                    @for assumption in assumptions {
                        @if let Some(text) = assumption.as_str() { li { (text) } }
                    }
                }
            }
            @if !next_steps.is_empty() {
                h3 { "Recommended next steps" }
                ol {
                    @for step in next_steps {
                        @if let Some(text) = step.as_str() { li { (text) } }
                    }
                }
            }
            p class="muted" {
                "This estimate is informational and will be confirmed during scoping."
            }
        }
    }
}

pub fn quote_error(message: &str) -> Markup {
    html! {
        p class="error" role="alert" { (message) }
    }
}

pub fn html_not_found() -> Markup {
    layout(
        "Not found",
        html! { main { h1 { "Not found" } p { "That application page does not exist." } } },
        None,
        None,
    )
}
