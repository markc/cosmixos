use dioxus::prelude::*;
use super::icons::*;

/// State for the compose form.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct ComposeState {
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
}

const ICON_BACK: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m15 18-6-6 6-6"/></svg>"#;

#[component]
pub fn ComposeView(
    state: ComposeState,
    on_back: EventHandler<()>,
    on_send: EventHandler<ComposeState>,
    on_discard: EventHandler<()>,
) -> Element {
    let mut to = use_signal(|| state.to.clone());
    let mut cc = use_signal(|| state.cc.clone());
    let mut bcc = use_signal(|| state.bcc.clone());
    let mut subject = use_signal(|| state.subject.clone());
    let mut body = use_signal(|| state.body.clone());
    let mut sending = use_signal(|| false);
    let in_reply_to = state.in_reply_to.clone();

    let can_send = !to().trim().is_empty() && !sending();

    rsx! {
        div {
            style: "flex:1; display:flex; flex-direction:column; min-width:0; overflow:hidden; background:#030712; height:100%;",
            // Header bar
            div {
                style: "flex-shrink:0; padding:12px 24px; border-bottom:1px solid #1f2937; background:rgba(17,24,39,0.3); display:flex; align-items:center; justify-content:space-between;",
                div {
                    style: "display:flex; align-items:center; gap:8px;",
                    button {
                        class: "mobile-back",
                        style: "display:none; background:none; border:none; color:#9ca3af; cursor:pointer; padding:4px;",
                        onclick: move |_| on_back.call(()),
                        dangerous_inner_html: "{ICON_BACK}"
                    }
                    span { style: "font-size:14px; font-weight:600; color:#f3f4f6;", "New Message" }
                }
                div {
                    style: "display:flex; gap:8px;",
                    // Discard
                    button {
                        style: "padding:6px 14px; background:none; border:1px solid #374151; color:#9ca3af; border-radius:5px; cursor:pointer; font-size:12px; display:flex; align-items:center; gap:4px;",
                        onclick: move |_| on_discard.call(()),
                        span { dangerous_inner_html: "{ICON_X}" }
                        "Discard"
                    }
                    // Send
                    button {
                        style: if can_send {
                            "padding:6px 14px; background:#2563eb; color:#fff; border:none; border-radius:5px; cursor:pointer; font-size:12px; font-weight:500; display:flex; align-items:center; gap:4px;"
                        } else {
                            "padding:6px 14px; background:#1e3a5f; color:#6b7280; border:none; border-radius:5px; cursor:not-allowed; font-size:12px; font-weight:500; display:flex; align-items:center; gap:4px;"
                        },
                        disabled: !can_send,
                        onclick: {
                            let in_reply_to = in_reply_to.clone();
                            move |_| {
                                sending.set(true);
                                on_send.call(ComposeState {
                                    to: to(),
                                    cc: cc(),
                                    bcc: bcc(),
                                    subject: subject(),
                                    body: body(),
                                    in_reply_to: in_reply_to.clone(),
                                });
                            }
                        },
                        span { dangerous_inner_html: "{ICON_SEND}" }
                        if sending() { "Sending..." } else { "Send" }
                    }
                }
            }
            // Form fields
            div {
                style: "flex-shrink:0; border-bottom:1px solid #1f2937;",
                // To
                div {
                    style: "display:flex; align-items:center; padding:0 24px; border-bottom:1px solid rgba(31,41,55,0.4);",
                    label { style: "width:50px; font-size:12px; color:#6b7280; flex-shrink:0;", "To" }
                    input {
                        style: "flex:1; background:transparent; border:none; outline:none; color:#e5e7eb; padding:10px 0; font-size:13px; font-family:inherit;",
                        r#type: "text",
                        value: "{to}",
                        placeholder: "recipient@example.com",
                        oninput: move |e| to.set(e.value()),
                    }
                }
                // Cc
                div {
                    style: "display:flex; align-items:center; padding:0 24px; border-bottom:1px solid rgba(31,41,55,0.4);",
                    label { style: "width:50px; font-size:12px; color:#6b7280; flex-shrink:0;", "Cc" }
                    input {
                        style: "flex:1; background:transparent; border:none; outline:none; color:#e5e7eb; padding:10px 0; font-size:13px; font-family:inherit;",
                        r#type: "text",
                        value: "{cc}",
                        oninput: move |e| cc.set(e.value()),
                    }
                }
                // Bcc
                div {
                    style: "display:flex; align-items:center; padding:0 24px; border-bottom:1px solid rgba(31,41,55,0.4);",
                    label { style: "width:50px; font-size:12px; color:#6b7280; flex-shrink:0;", "Bcc" }
                    input {
                        style: "flex:1; background:transparent; border:none; outline:none; color:#e5e7eb; padding:10px 0; font-size:13px; font-family:inherit;",
                        r#type: "text",
                        value: "{bcc}",
                        oninput: move |e| bcc.set(e.value()),
                    }
                }
                // Subject
                div {
                    style: "display:flex; align-items:center; padding:0 24px;",
                    label { style: "width:50px; font-size:12px; color:#6b7280; flex-shrink:0;", "Subject" }
                    input {
                        style: "flex:1; background:transparent; border:none; outline:none; color:#e5e7eb; padding:10px 0; font-size:13px; font-weight:500; font-family:inherit;",
                        r#type: "text",
                        value: "{subject}",
                        oninput: move |e| subject.set(e.value()),
                    }
                }
            }
            // Body
            div {
                style: "flex:1; overflow:hidden;",
                textarea {
                    style: "width:100%; height:100%; background:transparent; border:none; outline:none; color:#e5e7eb; padding:16px 24px; font-size:13px; font-family:system-ui,-apple-system,sans-serif; resize:none; line-height:1.6;",
                    value: "{body}",
                    placeholder: "Write your message...",
                    oninput: move |e| body.set(e.value()),
                }
            }
        }
    }
}

/// Build a ComposeState for replying to an email.
pub fn compose_reply(email: &crate::jmap::Email) -> ComposeState {
    let to = email
        .from
        .as_ref()
        .and_then(|addrs| addrs.first())
        .map(|a| a.email.clone())
        .unwrap_or_default();

    let subject = email
        .subject
        .as_deref()
        .map(|s| {
            if s.starts_with("Re: ") || s.starts_with("re: ") {
                s.to_string()
            } else {
                format!("Re: {s}")
            }
        })
        .unwrap_or_default();

    let quoted = email
        .text_body_value()
        .map(|text| {
            let from = email.from_display();
            let date = email.date_short();
            let mut q = format!("\n\nOn {date}, {from} wrote:\n");
            for line in text.lines() {
                q.push_str("> ");
                q.push_str(line);
                q.push('\n');
            }
            q
        })
        .unwrap_or_default();

    let in_reply_to = email
        .message_id
        .as_ref()
        .and_then(|ids| ids.first())
        .cloned();

    ComposeState {
        to,
        subject,
        body: quoted,
        in_reply_to,
        ..Default::default()
    }
}

/// Build a ComposeState for forwarding an email.
pub fn compose_forward(email: &crate::jmap::Email) -> ComposeState {
    let subject = email
        .subject
        .as_deref()
        .map(|s| {
            if s.starts_with("Fwd: ") || s.starts_with("fwd: ") {
                s.to_string()
            } else {
                format!("Fwd: {s}")
            }
        })
        .unwrap_or_default();

    let body = email
        .text_body_value()
        .map(|text| {
            let from = email.from_display();
            let date = email.date_short();
            let subj = email.subject.as_deref().unwrap_or("(no subject)");
            format!(
                "\n\n---------- Forwarded message ----------\nFrom: {from}\nDate: {date}\nSubject: {subj}\n\n{text}"
            )
        })
        .unwrap_or_default();

    ComposeState {
        subject,
        body,
        ..Default::default()
    }
}
