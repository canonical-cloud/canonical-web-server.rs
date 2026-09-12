//! Public readiness-training surface.
//!
//! This page intentionally lives in the Rust web server rather than making the
//! marketing site the product authority. It is static, credential-free content
//! that points into the existing authenticated readiness and quote flows.

use crate::AppState;
use axum::{
    http::{header, HeaderValue},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use maud::{html, Markup, DOCTYPE};

const TRAINING_CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; img-src 'self' data:; base-uri 'none'; form-action 'self'; frame-ancestors 'none'; object-src 'none'";
const TRAINING_CACHE_CONTROL: &str = "public, max-age=300, stale-while-revalidate=86400";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TrainingTrack {
    title: &'static str,
    format: &'static str,
    level: &'static str,
    summary: &'static str,
    outcomes: &'static [&'static str],
}

const TRAINING_TRACKS: &[TrainingTrack] = &[
    TrainingTrack {
        title: "SOC 2 Foundations",
        format: "4 modules · about 2 hours",
        level: "Beginner",
        summary: "Interactive fundamentals for teams that need a shared understanding of the Trust Services Criteria, system scope, control ownership, evidence, and the readiness journey.",
        outcomes: &[
            "Understand the SOC 2 readiness lifecycle and independent-assurance boundary.",
            "Identify the systems, people, vendors, and evidence that belong in scope.",
            "Recognize strong operating evidence versus policy-only evidence.",
        ],
    },
    TrainingTrack {
        title: "Control Owner Workshop",
        format: "6 modules · about 4 hours",
        level: "Intermediate",
        summary: "Role-based training for engineering, IT, security, and operations owners who need to operate controls continuously instead of preparing evidence at the last minute.",
        outcomes: &[
            "Translate control language into concrete engineering and operating responsibilities.",
            "Practice evidence capture, exception handling, review cadence, and ownership handoffs.",
            "Prepare control owners to answer readiness questions without over-claiming assurance.",
        ],
    },
    TrainingTrack {
        title: "Audit Readiness Bootcamp",
        format: "Hands-on cohort · scope-dependent",
        level: "Advanced",
        summary: "A guided sprint from gap assessment through evidence review and readiness rehearsal for teams approaching an independent SOC 2 examination.",
        outcomes: &[
            "Prioritize gaps by evidence quality, control design, and operating consistency.",
            "Rehearse evidence requests and management responses before independent review.",
            "Leave with a bounded remediation plan and named owners for unresolved readiness work.",
        ],
    },
];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/training", get(page))
        .route("/training/", get(page))
}

async fn page() -> Response {
    document(training_page())
}

fn document(markup: Markup) -> Response {
    let mut response = markup.into_response();
    for (name, value) in [
        (header::CONTENT_SECURITY_POLICY, TRAINING_CSP),
        (header::CACHE_CONTROL, TRAINING_CACHE_CONTROL),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::REFERRER_POLICY, "no-referrer"),
    ] {
        response
            .headers_mut()
            .insert(name, HeaderValue::from_static(value));
    }
    response
}

fn training_page() -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="color-scheme" content="light dark";
                meta name="description" content="Canonical Cloud readiness training for SaaS and AI teams preparing for SOC 2.";
                title { "Compliance readiness training · Canonical Cloud" }
                style {
                    "*{box-sizing:border-box}body{font-family:ui-sans-serif,system-ui,sans-serif;margin:0;line-height:1.55;background:#0b1020;color:#f7f8fc}a{color:inherit}.shell{max-width:76rem;margin:0 auto;padding:0 1.4rem}.site-nav{display:flex;justify-content:space-between;gap:1rem;align-items:center;padding:1.2rem 0}.site-nav .links{display:flex;gap:1rem;flex-wrap:wrap}.muted{color:#b8c0d8}.hero{padding:6rem 0 4rem}.eyebrow{font-weight:700;letter-spacing:.08em;text-transform:uppercase;color:#9fc1ff}.hero h1{font-size:clamp(2.7rem,7vw,5.7rem);line-height:1.01;max-width:14ch;margin:.6rem 0 1.2rem}.hero-copy{font-size:1.2rem;max-width:52rem;color:#d5daea}.actions{display:flex;gap:.8rem;flex-wrap:wrap;margin-top:1.8rem}.button{display:inline-block;text-decoration:none;border-radius:.7rem;padding:.8rem 1rem;font-weight:700;background:#f7f8fc;color:#0b1020}.button.secondary{background:transparent;color:#f7f8fc;border:1px solid #ffffff4d}.boundary{margin-top:1.25rem;padding:1rem;border-left:3px solid #8db4ff;background:#121a31}.section{padding:3.4rem 0}.section h2{font-size:clamp(2rem,4vw,3rem);margin:0 0 .8rem}.proof-grid,.track-grid,.steps-grid{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:1rem;margin-top:1.5rem}.proof,.track,.step,.faq{border:1px solid #ffffff24;background:#11192e;border-radius:1rem;padding:1.35rem}.proof strong{display:block;font-size:1.35rem}.track h3{font-size:1.45rem;margin:.2rem 0}.pill{display:inline-block;border:1px solid #8db4ff66;border-radius:999px;padding:.2rem .55rem;margin:.15rem .3rem .5rem 0;font-size:.9rem;color:#c8dcff}.track ul{padding-left:1.2rem}.step span{font-size:.82rem;text-transform:uppercase;letter-spacing:.08em;color:#9fc1ff;font-weight:700}.faq-stack{display:grid;gap:.8rem;margin-top:1.5rem}.faq h3{margin:.1rem 0 .4rem}.final-cta{margin:2rem 0 5rem;padding:2rem;border-radius:1.2rem;background:linear-gradient(135deg,#1d3970,#17213c)}footer{padding:2rem 0 3rem;border-top:1px solid #ffffff1f;color:#aeb7cf}@media(max-width:800px){.proof-grid,.track-grid,.steps-grid{grid-template-columns:1fr}.hero{padding-top:4rem}.site-nav{align-items:flex-start;flex-direction:column}}"
                }
            }
            body {
                div class="shell" {
                    nav class="site-nav" aria-label="Primary" {
                        a href="/" { strong { "canonical.cloud" } }
                        div class="links" {
                            a href="/training" aria-current="page" { "Training" }
                            a href="/app/readiness" { "Readiness assessment" }
                            a href="/u/quote" { "Talk to an expert" }
                        }
                    }

                    main {
                        section class="hero" {
                            p class="eyebrow" { "Canonical Cloud Training" }
                            h1 { "Turn compliance into a competitive advantage" }
                            p class="hero-copy" {
                                "Canonical Cloud helps SaaS and AI teams build toward SOC 2 readiness with practical training, evidence-focused exercises, expert guidance, and a clear path to independent review."
                            }
                            div class="actions" {
                                a class="button" href="/app/readiness" { "Start your readiness assessment" }
                                a class="button secondary" href="/u/quote" { "Talk to an expert" }
                            }
                            div class="boundary" role="note" {
                                strong { "Readiness, not assurance." }
                                " Training and readiness outputs help teams prepare. Canonical Cloud does not issue audit opinions, certifications, approvals, or guaranteed outcomes."
                            }
                        }

                        section class="section" aria-labelledby="why-training" {
                            p class="eyebrow" { "Built for operating teams" }
                            h2 id="why-training" { "Teach the people who actually operate the controls" }
                            p class="muted" { "The program is organized around concrete engineering and operating behavior rather than memorizing auditor terminology." }
                            div class="proof-grid" {
                                article class="proof" {
                                    strong { "Readiness-first" }
                                    span class="muted" { "Preparation for independent review without pretending training is the audit." }
                                }
                                article class="proof" {
                                    strong { "Role-based" }
                                    span class="muted" { "Founders, engineering, IT, security, and operations learn the responsibilities they own." }
                                }
                                article class="proof" {
                                    strong { "SOC 2 Type I & II preparation" }
                                    span class="muted" { "Training covers readiness concepts that matter before design-date and operating-period review." }
                                }
                            }
                        }

                        section class="section" aria-labelledby="tracks" {
                            p class="eyebrow" { "Training paths" }
                            h2 id="tracks" { "Start with the level your team needs" }
                            p class="muted" { "Take one track or use all three as a progression from shared vocabulary to hands-on readiness rehearsal." }
                            div class="track-grid" {
                                @for track in TRAINING_TRACKS {
                                    article class="track" {
                                        div {
                                            span class="pill" { (track.level) }
                                            span class="pill" { (track.format) }
                                        }
                                        h3 { (track.title) }
                                        p { (track.summary) }
                                        h4 { "You will practice" }
                                        ul {
                                            @for outcome in track.outcomes {
                                                li { (outcome) }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        section class="section" aria-labelledby="pathway" {
                            p class="eyebrow" { "From learning to evidence" }
                            h2 id="pathway" { "Training connects directly to the readiness workflow" }
                            div class="steps-grid" {
                                article class="step" {
                                    span { "01 · Learn" }
                                    h3 { "Build a shared model" }
                                    p class="muted" { "Align on scope, control intent, evidence quality, and the boundary between readiness and assurance." }
                                }
                                article class="step" {
                                    span { "02 · Operate" }
                                    h3 { "Assign owners and practice" }
                                    p class="muted" { "Turn requirements into recurring engineering and operational actions with named owners and review cadence." }
                                }
                                article class="step" {
                                    span { "03 · Rehearse" }
                                    h3 { "Run a readiness assessment" }
                                    p class="muted" { "Use the authenticated readiness worksheets to identify gaps before an independent evaluator asks for evidence." }
                                }
                            }
                        }

                        section class="section" aria-labelledby="faq" {
                            p class="eyebrow" { "Questions" }
                            h2 id="faq" { "Keep the assurance boundary explicit" }
                            div class="faq-stack" {
                                article class="faq" {
                                    h3 { "Does the training certify us as SOC 2 compliant?" }
                                    p class="muted" { "No. SOC 2 is independent assurance work. Training helps your team prepare systems, controls, evidence, and operating practices for that process." }
                                }
                                article class="faq" {
                                    h3 { "Who should attend?" }
                                    p class="muted" { "Founders and program leads can start with Foundations; engineering, IT, security, and operations control owners benefit most from the workshop and bootcamp." }
                                }
                                article class="faq" {
                                    h3 { "Can we use the training before choosing an auditor?" }
                                    p class="muted" { "Yes. The material is designed for readiness and remediation. Any eventual examination scope and evidence requests remain the responsibility of your independent evaluator." }
                                }
                            }
                        }

                        section class="final-cta" aria-labelledby="training-next" {
                            p class="eyebrow" { "Next step" }
                            h2 id="training-next" { "Turn the lessons into a concrete readiness plan" }
                            p { "Start with the readiness worksheet, or open a scoped conversation about training and remediation for your team." }
                            div class="actions" {
                                a class="button" href="/app/readiness" { "Start your readiness assessment" }
                                a class="button secondary" href="/u/quote" { "Talk to an expert" }
                            }
                        }
                    }

                    footer {
                        "Canonical Cloud · readiness education and compliance operations for technology teams."
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_preserves_the_three_confirmed_training_tracks() {
        assert_eq!(TRAINING_TRACKS.len(), 3);
        assert_eq!(TRAINING_TRACKS[0].title, "SOC 2 Foundations");
        assert_eq!(TRAINING_TRACKS[0].format, "4 modules · about 2 hours");
        assert_eq!(TRAINING_TRACKS[1].title, "Control Owner Workshop");
        assert_eq!(TRAINING_TRACKS[1].format, "6 modules · about 4 hours");
        assert_eq!(TRAINING_TRACKS[2].title, "Audit Readiness Bootcamp");
    }

    #[test]
    fn page_is_readiness_first_and_avoids_unsubstantiated_result_metrics() {
        let rendered = training_page().into_string();
        assert!(rendered.contains("Turn compliance into a competitive advantage"));
        assert!(rendered.contains("Start your readiness assessment"));
        assert!(rendered.contains("Talk to an expert"));
        assert!(rendered.contains("Readiness, not assurance."));
        assert!(rendered.contains(
            "does not issue audit opinions, certifications, approvals, or guaranteed outcomes"
        ));
        assert!(!rendered.contains("85% faster"));
        assert!(!rendered.contains("guaranteed clean"));
    }

    #[tokio::test]
    async fn public_training_document_has_a_narrow_static_policy() {
        let response = page().await;
        assert_eq!(
            response.headers()[header::CONTENT_SECURITY_POLICY],
            TRAINING_CSP
        );
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            TRAINING_CACHE_CONTROL
        );
        assert_eq!(
            response.headers()[header::X_CONTENT_TYPE_OPTIONS],
            "nosniff"
        );
        assert!(!TRAINING_CSP.contains("unsafe-eval"));
        assert!(!TRAINING_CSP.contains("script-src"));
    }
}
