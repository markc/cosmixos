mod jmap;

use dioxus::prelude::*;
use jmap::{Email, JmapClient, Mailbox};

const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

const JMAP_URL: &str = "https://172.16.2.4:8443";
const JMAP_USER: &str = "markc@goldcoast.org";
const JMAP_PASS: &str = "changeme_N0W";

fn main() {
    #[cfg(target_os = "linux")]
    unsafe {
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        // Force GTK dark theme for window chrome
        std::env::set_var("GTK_THEME", "Adwaita:dark");
    };

    #[cfg(feature = "desktop")]
    {
        use dioxus_desktop::{Config, WindowBuilder};
        use tao::window::Theme;

        let cfg = Config::new()
            .with_window(
                WindowBuilder::new()
                    .with_title("Cosmix Mail")
                    .with_inner_size(dioxus_desktop::LogicalSize::new(1400.0, 900.0))
                    .with_theme(Some(Theme::Dark))
                    .with_decorations(true)
            )
            .with_background_color((3, 7, 18, 255));

        LaunchBuilder::new().with_cfg(cfg).launch(app);
        return;
    }

    #[allow(unreachable_code)]
    dioxus::launch(app);
}

fn app() -> Element {
    let client = use_signal(|| JmapClient::new(JMAP_URL, JMAP_USER, JMAP_PASS).unwrap());
    let mut mailboxes: Signal<Vec<Mailbox>> = use_signal(Vec::new);
    let mut selected_mailbox: Signal<Option<String>> = use_signal(|| None);
    let mut emails: Signal<Vec<Email>> = use_signal(Vec::new);
    let mut selected_email: Signal<Option<Email>> = use_signal(|| None);
    let mut error_msg: Signal<Option<String>> = use_signal(|| None);

    // Load mailboxes on startup
    let _load_mailboxes = use_resource(move || {
        let c = client.peek().clone();
        async move {
            match c.mailboxes().await {
                Ok(mut mboxes) => {
                    mboxes.sort_by(|a, b| {
                        let role_order = |r: &Option<String>| match r.as_deref() {
                            Some("inbox") => 0,
                            Some("drafts") => 1,
                            Some("sent") => 2,
                            Some("archive") => 3,
                            Some("junk") => 4,
                            Some("trash") => 5,
                            _ => 6,
                        };
                        role_order(&a.role).cmp(&role_order(&b.role))
                    });
                    if let Some(inbox) = mboxes.iter().find(|m| m.role.as_deref() == Some("inbox")) {
                        selected_mailbox.set(Some(inbox.id.clone()));
                    }
                    mailboxes.set(mboxes);
                }
                Err(e) => error_msg.set(Some(format!("Failed to load mailboxes: {e}"))),
            }
        }
    });

    // Load emails when mailbox changes
    let _load_emails = use_resource(move || {
        let c = client.peek().clone();
        async move {
            let Some(mbox_id) = selected_mailbox() else {
                return;
            };
            selected_email.set(None);
            match c.email_ids(&mbox_id).await {
                Ok(ids) => match c.emails(&ids, false).await {
                    Ok(list) => emails.set(list),
                    Err(e) => error_msg.set(Some(format!("Failed to load emails: {e}"))),
                },
                Err(e) => error_msg.set(Some(format!("Failed to query emails: {e}"))),
            }
        }
    });

    rsx! {
        document::Link { rel: "stylesheet", href: TAILWIND_CSS }
        // Force zero margin on body/html — must be a <style> in head to beat browser defaults
        document::Style { "html,body,#main{{ margin:0!important; padding:0!important; background:#030712!important; width:100%!important; height:100%!important; overflow:hidden!important; }}" }
        // Three-pane layout using inline styles for bulletproof positioning
        div {
            style: "position:absolute; top:0; left:0; right:0; bottom:0; display:flex; flex-direction:row; overflow:hidden; background:#030712; color:#e5e7eb; font-size:13px; font-family:system-ui,-apple-system,sans-serif;",
            // Error banner
            if let Some(err) = error_msg() {
                div {
                    style: "position:fixed; top:0; left:0; right:0; background:rgba(127,29,29,0.95); color:#fecaca; padding:6px 16px; font-size:12px; z-index:50; display:flex; align-items:center; justify-content:space-between;",
                    span { "{err}" }
                    button {
                        style: "margin-left:12px; cursor:pointer; background:none; border:none; color:inherit;",
                        onclick: move |_| error_msg.set(None),
                        dangerous_inner_html: "{ICON_X}"
                    }
                }
            }

            // Pane 1: Sidebar — mailboxes
            MailboxList {
                mailboxes: mailboxes(),
                selected: selected_mailbox(),
                on_select: move |id: String| {
                    selected_mailbox.set(Some(id));
                }
            }

            // Pane 2: Email list
            EmailList {
                emails: emails(),
                selected_id: selected_email().map(|e| e.id.clone()),
                on_select: move |email: Email| {
                    let c = client.peek().clone();
                    spawn(async move {
                        match c.emails(&[email.id.clone()], true).await {
                            Ok(full) => {
                                if let Some(e) = full.into_iter().next() {
                                    selected_email.set(Some(e));
                                }
                            }
                            Err(e) => error_msg.set(Some(format!("Failed to load email: {e}"))),
                        }
                    });
                }
            }

            // Pane 3: Email view
            EmailView { email: selected_email() }
        }
    }
}

// --- Lucide SVG icons (24x24, stroke-based) ---

const ICON_INBOX: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="22 12 16 12 14 15 10 15 8 12 2 12"/><path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/></svg>"#;
const ICON_FILE_EDIT: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.375 2.625a1 1 0 0 1 3 3l-9.013 9.014a2 2 0 0 1-.853.505l-2.873.84a.5.5 0 0 1-.62-.62l.84-2.873a2 2 0 0 1 .506-.852z"/></svg>"#;
const ICON_SEND: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14.536 21.686a.5.5 0 0 0 .937-.024l6.5-19a.496.496 0 0 0-.635-.635l-19 6.5a.5.5 0 0 0-.024.937l7.93 3.18a2 2 0 0 1 1.112 1.11z"/><path d="m21.854 2.147-10.94 10.939"/></svg>"#;
const ICON_ARCHIVE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="20" height="5" x="2" y="3" rx="1"/><path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8"/><path d="M10 12h4"/></svg>"#;
const ICON_ALERT_TRIANGLE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/></svg>"#;
const ICON_TRASH: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/></svg>"#;
const ICON_FOLDER: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2z"/></svg>"#;
const ICON_MAIL: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="20" height="16" x="2" y="4" rx="2"/><path d="m22 7-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 7"/></svg>"#;
const ICON_PAPERCLIP: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48"/></svg>"#;
const ICON_X: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>"#;
const ICON_USER: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>"#;
const ICON_CLOCK: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>"#;

fn mailbox_icon(role: Option<&str>) -> &'static str {
    match role {
        Some("inbox") => ICON_INBOX,
        Some("drafts") => ICON_FILE_EDIT,
        Some("sent") => ICON_SEND,
        Some("archive") => ICON_ARCHIVE,
        Some("junk") => ICON_ALERT_TRIANGLE,
        Some("trash") => ICON_TRASH,
        _ => ICON_FOLDER,
    }
}

#[component]
fn MailboxList(mailboxes: Vec<Mailbox>, selected: Option<String>, on_select: EventHandler<String>) -> Element {
    rsx! {
        div {
            style: "width:200px; min-width:200px; display:flex; flex-direction:column; background:#111827; border-right:1px solid #1f2937; height:100%;",
            // App title
            div {
                style: "height:44px; display:flex; align-items:center; padding:0 14px; border-bottom:1px solid #1f2937; gap:8px;",
                span { dangerous_inner_html: "{ICON_MAIL}" }
                span { style: "font-weight:600; color:#f3f4f6;", "cosmix" }
            }
            // Mailbox list
            nav {
                style: "flex:1; overflow-y:auto; padding:6px 0;",
                for mbox in mailboxes {
                    {
                        let id = mbox.id.clone();
                        let is_selected = selected.as_deref() == Some(&id);
                        let icon = mailbox_icon(mbox.role.as_deref());
                        let bg = if is_selected { "background:#2563eb; color:#fff;" } else { "color:#9ca3af;" };
                        rsx! {
                            button {
                                key: "{id}",
                                style: "width:calc(100% - 8px); text-align:left; padding:5px 12px; margin:1px 4px; border-radius:4px; display:flex; align-items:center; gap:8px; font-size:12px; border:none; cursor:pointer; {bg}",
                                onclick: move |_| on_select.call(id.clone()),
                                span { dangerous_inner_html: "{icon}" }
                                span { style: "overflow:hidden; text-overflow:ellipsis; white-space:nowrap;", "{mbox.name}" }
                            }
                        }
                    }
                }
            }
            // Account
            div {
                style: "padding:8px 14px; border-top:1px solid #1f2937; font-size:11px; color:#6b7280; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;",
                "{JMAP_USER}"
            }
        }
    }
}

#[component]
fn EmailList(emails: Vec<Email>, selected_id: Option<String>, on_select: EventHandler<Email>) -> Element {
    rsx! {
        div {
            style: "width:300px; min-width:300px; display:flex; flex-direction:column; border-right:1px solid #1f2937; background:#0d1117; height:100%;",
            // Header
            div {
                style: "height:44px; display:flex; align-items:center; padding:0 16px; border-bottom:1px solid #1f2937; font-size:11px; color:#6b7280;",
                "{emails.len()} messages"
            }
            // Message list
            div {
                style: "flex:1; overflow-y:auto;",
                if emails.is_empty() {
                    div {
                        style: "display:flex; align-items:center; justify-content:center; height:100px; color:#4b5563; font-size:12px;",
                        "No messages"
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

#[component]
fn EmailView(email: Option<Email>) -> Element {
    let Some(email) = email else {
        return rsx! {
            div {
                style: "flex:1; display:flex; align-items:center; justify-content:center; color:#4b5563; height:100%;",
                div { style: "text-align:center;",
                    div { style: "margin-bottom:8px; opacity:0.3;", dangerous_inner_html: r#"<svg xmlns="http://www.w3.org/2000/svg" width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1" stroke-linecap="round" stroke-linejoin="round"><rect width="20" height="16" x="2" y="4" rx="2"/><path d="m22 7-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 7"/></svg>"# }
                    span { style: "font-size:12px;", "Select a message to read" }
                }
            }
        };
    };

    let body_html = if let Some(html) = email.html_body_value() {
        html.to_string()
    } else if let Some(text) = email.text_body_value() {
        render_markdown(text)
    } else {
        "<p style=\"color:#6b7280\">No content</p>".to_string()
    };

    let to_display = email
        .to
        .as_ref()
        .map(|addrs| {
            addrs
                .iter()
                .map(|a| a.email.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();

    rsx! {
        div {
            style: "flex:1; display:flex; flex-direction:column; min-width:0; overflow:hidden; background:#030712; height:100%;",
            // Email header
            div {
                style: "flex-shrink:0; padding:16px 24px; border-bottom:1px solid #1f2937; background:rgba(17,24,39,0.3);",
                h2 { style: "font-size:15px; font-weight:600; color:#f3f4f6; margin-bottom:8px;",
                    "{email.subject.as_deref().unwrap_or(\"(no subject)\")}"
                }
                div {
                    style: "display:flex; flex-wrap:wrap; gap:12px; font-size:12px; color:#6b7280; align-items:center;",
                    span { style: "display:inline-flex; align-items:center; gap:4px;",
                        span { dangerous_inner_html: "{ICON_USER}" }
                        span { style: "color:#d1d5db;", "{email.from_display()}" }
                    }
                    span { style: "display:inline-flex; align-items:center; gap:4px;",
                        "To: "
                        span { style: "color:#9ca3af;", "{to_display}" }
                    }
                    span { style: "display:inline-flex; align-items:center; gap:4px;",
                        span { dangerous_inner_html: "{ICON_CLOCK}" }
                        "{email.date_short()}"
                    }
                    if email.has_attachment.unwrap_or(false) {
                        span { style: "display:inline-flex; align-items:center; gap:4px; color:#f59e0b;",
                            span { dangerous_inner_html: "{ICON_PAPERCLIP}" }
                            "attachment"
                        }
                    }
                }
            }
            // Email body
            div {
                style: "flex:1; overflow-y:auto; padding:20px 24px;",
                div {
                    class: "prose",
                    style: "max-width:768px;",
                    dangerous_inner_html: "{body_html}"
                }
            }
        }
    }
}

fn render_markdown(text: &str) -> String {
    use pulldown_cmark::{Options, Parser, html};
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(text, opts);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    html_output
}
