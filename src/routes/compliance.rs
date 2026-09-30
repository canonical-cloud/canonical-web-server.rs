use crate::{
    auth::{AuthContext, SessionAuthenticated},
    error::AppError,
    AppState,
};
use axum::{
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use maud::{html, Markup, DOCTYPE};

#[derive(Clone, Copy, Debug)]
struct Capability {
    slug: &'static str,
    key: &'static str,
    title: &'static str,
    summary: &'static str,
    api_path: &'static str,
    tables: &'static [&'static str],
    workflows: &'static [&'static str],
}

const CAPABILITIES: &[Capability] = &[
    Capability {
        slug: "controls",
        key: "controls_evidence",
        title: "Controls & evidence",
        summary: "Control ownership, implementation state, evidence freshness, exceptions, automated tests, and remediation.",
        api_path: "/v1/grc/controls",
        tables: &["control_implementations", "control_exceptions", "control_tests", "control_test_runs", "evidence_records", "evidence_control_links"],
        workflows: &["Assign control owners", "Collect and review evidence", "Run continuous tests", "Approve exceptions", "Track remediation"],
    },
    Capability {
        slug: "frameworks",
        key: "framework_crosswalks",
        title: "Frameworks & crosswalks",
        summary: "Run multiple frameworks from reusable control mappings while preserving framework-specific requirements.",
        api_path: "/v1/grc/frameworks",
        tables: &["framework_releases", "control_references", "audit_engagement_frameworks", "framework_coverage_snapshots"],
        workflows: &["Select frameworks", "Map shared controls", "Review coverage", "Track framework releases"],
    },
    Capability {
        slug: "policies",
        key: "policies_people_training",
        title: "Policies, people & training",
        summary: "Version policies, collect approvals and acknowledgements, and track personnel and compliance training.",
        api_path: "/v1/grc/policies",
        tables: &["policies", "policy_versions", "policy_acknowledgements", "personnel_records", "training_courses", "training_assignments"],
        workflows: &["Draft and approve policies", "Publish policy versions", "Collect acknowledgements", "Assign training", "Track overdue training"],
    },
    Capability {
        slug: "risks",
        key: "risk_management",
        title: "Risk management",
        summary: "Maintain inherent and residual risk, mapped controls, treatment plans, acceptance, and review cadence.",
        api_path: "/v1/grc/risks",
        tables: &["risks", "risk_controls", "risk_treatments"],
        workflows: &["Register risks", "Score inherent risk", "Map mitigating controls", "Approve treatment or acceptance", "Review residual risk"],
    },
    Capability {
        slug: "assets",
        key: "asset_vulnerability",
        title: "Assets & vulnerabilities",
        summary: "Inventory systems and relationships, ingest vulnerability observations, and track remediation to closure.",
        api_path: "/v1/grc/assets",
        tables: &["assets", "asset_relationships", "vulnerabilities"],
        workflows: &["Discover assets", "Assign owners and criticality", "Ingest vulnerabilities", "Prioritize remediation", "Verify resolution"],
    },
    Capability {
        slug: "vendors",
        key: "vendor_risk",
        title: "Vendor risk",
        summary: "Track vendor lifecycle, due diligence, evidence, findings, continuous monitoring, and reassessment.",
        api_path: "/v1/grc/vendors",
        tables: &["vendors", "vendor_assessments", "vendor_assessment_evidence", "vendor_monitor_events"],
        workflows: &["Onboard and tier vendors", "Run due diligence", "Review vendor evidence", "Monitor material events", "Reassess and offboard"],
    },
    Capability {
        slug: "access-reviews",
        key: "access_reviews",
        title: "Access reviews",
        summary: "Run recurring entitlement reviews with explicit approve, revoke, modify, and remediation decisions.",
        api_path: "/v1/grc/access-reviews",
        tables: &["access_review_campaigns", "access_review_items"],
        workflows: &["Collect grants", "Launch review campaigns", "Record reviewer decisions", "Revoke or modify access", "Retain evidence"],
    },
    Capability {
        slug: "audits",
        key: "audit_collaboration",
        title: "Audit collaboration",
        summary: "Coordinate engagements, phases, evidence requests, findings, recommendations, and independent-review handoff.",
        api_path: "/v1/grc/audits",
        tables: &["audit_engagements", "audit_phases", "audit_evidence_requests", "audit_evidence_request_items", "findings", "recommendations"],
        workflows: &["Open an engagement", "Manage PBC requests", "Submit evidence", "Resolve findings", "Prepare reviewer handoff"],
    },
    Capability {
        slug: "trust-center",
        key: "trust_center",
        title: "Trust center",
        summary: "Publish approved security and compliance resources with public, gated, NDA, and approved-only access.",
        api_path: "/v1/grc/trust-centers",
        tables: &["trust_centers", "trust_center_resources", "trust_center_access_requests"],
        workflows: &["Publish approved resources", "Request access", "Enforce NDA gates", "Approve or revoke access", "Retire stale material"],
    },
    Capability {
        slug: "questionnaires",
        key: "security_questionnaires",
        title: "Security questionnaires",
        summary: "Ingest questionnaires, assign questions, draft grounded answers, require review, and preserve answer provenance.",
        api_path: "/v1/grc/questionnaires",
        tables: &["security_questionnaires", "security_questionnaire_questions", "security_questionnaire_answers", "questionnaire_answer_sources", "knowledge_base_entries"],
        workflows: &["Import questionnaires", "Assign questions", "Suggest approved-source answers", "Review and approve answers", "Export responses"],
    },
    Capability {
        slug: "automations",
        key: "integrations_automation",
        title: "Integrations & automation",
        summary: "Connect least-privilege sources, schedule collections, retain capability snapshots, and surface drift or failed runs.",
        api_path: "/v1/grc/automations",
        tables: &["integration_connections", "connection_permission_snapshots", "automation_jobs", "automation_runs"],
        workflows: &["Connect providers", "Verify permissions", "Schedule evidence collection", "Review failed runs", "Detect capability drift"],
    },
    Capability {
        slug: "ai-governance",
        key: "ai_governance",
        title: "AI governance",
        summary: "Inventory models and agents, record use cases and data classes, assign risk tiers, map controls, and monitor events.",
        api_path: "/v1/grc/ai-systems",
        tables: &["ai_systems", "ai_system_controls", "ai_monitor_events"],
        workflows: &["Register AI systems", "Classify risk and data", "Assign human oversight", "Map controls", "Monitor drift and events"],
    },
    Capability {
        slug: "subscription",
        key: "commercial_entitlements",
        title: "Plans & entitlements",
        summary: "Keep pricing, subscriptions, product features, and runtime entitlements connected to an explicit commercial source of truth.",
        api_path: "/v1/grc/subscription",
        tables: &["commercial_plan_catalog", "commercial_plan_features", "tenant_subscriptions"],
        workflows: &["View plan", "Resolve entitlements", "Apply feature limits", "Track subscription state"],
    },
];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/compliance", get(index))
        .route("/compliance/{slug}", get(detail))
}

async fn index(auth: Result<SessionAuthenticated, AppError>) -> Response {
    match auth {
        Ok(SessionAuthenticated(actor)) => render_index(&actor).into_response(),
        Err(AppError::Unauthorized) => Redirect::to("/login").into_response(),
        Err(error) => error.into_response(),
    }
}

async fn detail(
    auth: Result<SessionAuthenticated, AppError>,
    Path(slug): Path<String>,
) -> Response {
    let actor = match auth {
        Ok(SessionAuthenticated(actor)) => actor,
        Err(AppError::Unauthorized) => return Redirect::to("/login").into_response(),
        Err(error) => return error.into_response(),
    };
    let Some(capability) = CAPABILITIES.iter().find(|item| item.slug == slug) else {
        return (StatusCode::NOT_FOUND, "compliance module not found").into_response();
    };
    render_detail(&actor, capability).into_response()
}

fn shell(actor: &AuthContext, title: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) " · Canonical Plus" }
                style {
                    "body{font-family:ui-sans-serif,system-ui,sans-serif;max-width:76rem;margin:0 auto;padding:2rem;line-height:1.5}nav{display:flex;justify-content:space-between;gap:1rem;flex-wrap:wrap}main{margin-top:2rem}.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(18rem,1fr));gap:1rem}.card{border:1px solid #8886;border-radius:.75rem;padding:1.25rem}.muted{opacity:.7}.pill{display:inline-block;border:1px solid #8886;border-radius:99rem;padding:.15rem .55rem;margin:.1rem .2rem .1rem 0;font-size:.85rem}code{overflow-wrap:anywhere}"
                }
            }
            body {
                nav {
                    a href="/app" { strong { "Canonical Plus" } }
                    span {
                        a href="/app/compliance" { "Compliance" }
                        " · "
                        a href="/app/engagements" { "Engagements" }
                        " · "
                        a href="/app/readiness" { "Readiness" }
                    }
                }
                (body)
                footer class="muted" { "Signed in as " (actor.email) }
            }
        }
    }
}

fn render_index(actor: &AuthContext) -> Markup {
    shell(
        actor,
        "Compliance platform",
        html! {
            main {
                h1 { "Compliance platform" }
                p { "A single operating surface for continuous controls, evidence, risk, vendors, audits, trust, questionnaires, automation, and AI governance." }
                p class="muted" { "These modules describe the admitted Canonical GRC contract and persistence model. Individual write workflows remain unavailable until their typed API operation and authorization tests are admitted." }
                div class="grid" {
                    @for capability in CAPABILITIES {
                        article class="card" {
                            h2 { a href={ "/app/compliance/" (capability.slug) } { (capability.title) } }
                            p { (capability.summary) }
                            p class="muted" { code { (capability.key) } }
                        }
                    }
                }
            }
        },
    )
}

fn render_detail(actor: &AuthContext, capability: &Capability) -> Markup {
    shell(
        actor,
        capability.title,
        html! {
            main {
                p { a href="/app/compliance" { "← Compliance platform" } }
                h1 { (capability.title) }
                p { (capability.summary) }
                section class="card" {
                    h2 { "Workflow target" }
                    ul {
                        @for workflow in capability.workflows {
                            li { (workflow) }
                        }
                    }
                }
                section class="card" {
                    h2 { "Typed API boundary" }
                    p { code { (capability.api_path) } }
                    p class="muted" { "The path is reserved by the GRC contract. A write operation is not considered live merely because the module appears here." }
                }
                section class="card" {
                    h2 { "Persistence boundary" }
                    p class="muted" { "Private ORM tables; application code must use typed domain operations rather than generic SQL." }
                    div {
                        @for table in capability.tables {
                            span class="pill" { code { (table) } }
                        }
                    }
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::CAPABILITIES;

    #[test]
    fn compliance_modules_match_the_bounded_grc_contract_surface() {
        assert_eq!(CAPABILITIES.len(), 13);
        let slugs = CAPABILITIES.iter().map(|item| item.slug).collect::<BTreeSet<_>>();
        let keys = CAPABILITIES.iter().map(|item| item.key).collect::<BTreeSet<_>>();
        assert_eq!(slugs.len(), CAPABILITIES.len());
        assert_eq!(keys.len(), CAPABILITIES.len());
        for item in CAPABILITIES {
            assert!(item.api_path.starts_with("/v1/grc/"));
            assert!(!item.tables.is_empty());
            assert!(!item.tables.iter().any(|table| table.contains("secret")));
        }
    }
}
