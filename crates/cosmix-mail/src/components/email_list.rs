use dioxus::prelude::*;
use crate::jmap::Email;
use super::icons::*;
use cosmix_ui::icons::ICON_MENU;

#[component]
pub fn EmailList(
    class: String,
    emails: Vec<Email>,
    selected_id: Option<String>,
    on_select: EventHandler<Email>,
    on_menu: EventHandler<()>,
) -> Element {
    rsx! {
        div {
            class: "{class}",
            style: "width:300px; min-width:300px; display:flex; flex-direction:column; border-right:1px solid #1f2937; background:#0d1117; height:100%;",
            // Header
            div {
                style: "height:44px; display:flex; align-items:center; padding:0 16px; border-bottom:1px solid #1f2937; font-size:11px; color:#6b7280; gap:8px;",
                // Mobile hamburger menu
                button {
                    class: "mobile-back",
                    style: "display:none; background:none; border:none; color:#9ca3af; cursor:pointer; padding:4px;",
                    onclick: move |_| on_menu.call(()),
                    dangerous_inner_html: "{ICON_MENU}"
                }
                "{emails.len()} messages"
            }
            // Message list
            div {
                style: "flex:1; overflow-y:auto;",
                if emails.is_empty() {
                    div {
                        style: "display:flex; flex-direction:column; align-items:center; justify-content:center; height:200px; color:#4b5563; font-size:12px; gap:8px;",
                        span { dangerous_inner_html: "{ICON_MAIL}" }
                        "No messages in this mailbox"
                    }
                }
                for email in emails {
                    {
                        let is_selected = selected_id.as_deref() == Some(&email.id);
                        let is_read = email.is_read();
                        let email_clone = email.clone();
                        let bg = if is_selected {
                            "background:rgba(30,58,138,0.3); border-left:2px solid #3b82f6;"
                        } else {
                            "border-left:2px solid transparent;"
                        };
                        let from_style = if is_read {
                            "font-size:12px; color:#6b7280; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;"
                        } else {
                            "font-size:12px; color:#e5e7eb; font-weight:600; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;"
                        };
                        let subj_style = if is_read {
                            "font-size:12px; color:#9ca3af; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; margin-top:2px;"
                        } else {
                            "font-size:12px; color:#f3f4f6; font-weight:500; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; margin-top:2px;"
                        };
                        rsx! {
                            button {
                                key: "{email.id}",
                                style: "width:100%; text-align:left; padding:8px 14px; border:none; border-bottom:1px solid rgba(31,41,55,0.5); cursor:pointer; {bg}",
                                onclick: move |_| on_select.call(email_clone.clone()),
                                // From + date
                                div {
                                    style: "display:flex; justify-content:space-between; align-items:baseline; gap:8px;",
                                    span { style: "{from_style}", "{email.from_display()}" }
                                    span { style: "font-size:10px; color:#4b5563; flex-shrink:0;", "{email.date_short()}" }
                                }
                                // Subject
                                div { style: "{subj_style}",
                                    "{email.subject.as_deref().unwrap_or(\"(no subject)\")}"
                                }
                                // Preview
                                div {
                                    style: "font-size:11px; color:#4b5563; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; margin-top:2px; line-height:1.3;",
                                    "{email.preview.as_deref().unwrap_or(\"\")}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
