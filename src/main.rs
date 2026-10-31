use std::{cell::RefCell, net::TcpStream, process::Command, rc::Rc, sync::Arc, sync::Mutex};

use gtk::prelude::*;
use gtk4::{self as gtk, ScrolledWindow, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use ollama_rs::{
    Ollama,
    generation::chat::{ChatMessage, request::ChatMessageRequest},
};
use serde_json::{json, Value};
mod ollama_runner;

const CSS: &str = "

window.copilot {
    background: transparent;
    min-width: 0px;
    margin-top: 0px;
    
    
    transition: min-width 0.6s cubic-bezier(0.68, -0.6, 0.32, 1.6), margin-top 0.5s cubic-bezier(.42,1.69,.68,.86);
    border-radius: 12px 12px 12px 12px;
}

window.expanded {
    min-width: 500px;
    margin-top: 250px;
}

window.locked {
    transition: min-width 0.0s cubic-bezier(0.68, -0.6, 0.32, 1.6), margin-top 0.0s cubic-bezier(.42,1.69,.68,.86);
    min-width: 500px;
}

window.expanded entry {
}

window.copilot entry {
    // transform: translateY(40px); 

    padding: 12px 16px;
    font-size: 18px;
    background: #000000;
    color: #eceff4;
    border: 0px solid #5e81ac;
    box-shadow: 0 8px 24px rgba(0,0,0,0.0);

}
window.copilot text {
    background-color: #000000;
    color: #ffffff;
}

window.copilot text selection {
    background-color: #2f2f2f;
    color: #ffffff;
}

entry.promptExpanded,
entry.promptExpanded:focus-within {
    border-radius: 12px 12px 0px 0px;
}

.response{
    font-size: 0px;
    border-radius: 0px 0px 12px 12px;
    min-height: 0px;
    transition: min-height 0.6s cubic-bezier(0.68, -0.6, .2, 1.6);
    // transform: scaleY(0); 
    // transform-origin: top; 
}

.responseExpander {
    // transform: scaleY(1); 

    min-height: 50px;
    font-size: 18px;
    padding: 12px 16px;
    background: #000000;
    border: 0px solid #5e81ac;
    box-shadow: 0 8px 24px rgba(0,0,0,0.0);
}

.responseExpanded {
    min-height: 100px;
    background: #000000;
    padding: 12px 16px;
    font-size: 18px;
    }
";

/// Get the current weather for a specific location.
///
/// # Arguments
/// * `location` - The city e.g. San Francisco,CA,US or London
#[ollama_rs::function]
async fn get_weather(location: String) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    _ = location;
    Ok("".to_owned())
}

#[tokio::main]
async fn main() -> gtk::glib::ExitCode {
    if !TcpStream::connect(format!("{}:{}", "localhost", 11434)).is_ok() {
        let mut command = {
            let mut cmd = Command::new("ollama");
            cmd.arg("serve");
            cmd
        };

        let _ = command.spawn().expect("failed to execute");
    }
    let app = gtk::Application::builder()
        .application_id("me.ai.copilot")
        .build();
    app.connect_activate(build_ui);
    let res = app.run();

    res
}

fn build_ui(app: &gtk::Application) {
    let chat_context: Arc<Mutex<Vec<ChatMessage>>> = Arc::new(Mutex::new(vec![ChatMessage::system("You are an AI model developed by the user, you are currently running locally in a dev mode".to_owned())]));

    // Load CSS for the whole display
    let provider = gtk::CssProvider::new();
    provider.load_from_data(CSS);
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().unwrap(),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let win = gtk::ApplicationWindow::new(app);
    win.add_css_class("copilot");
    win.set_default_size(0, -1);
    win.set_modal(false);

    win.init_layer_shell();
    win.set_layer(Layer::Overlay);
    win.set_keyboard_mode(KeyboardMode::OnDemand);
    win.set_anchor(Edge::Top, true);
    win.set_margin(Edge::Top, 0);

    let widget_box = gtk::Box::new(gtk4::Orientation::Vertical, 0);

    let entry = gtk::Entry::builder().placeholder_text("").build();

    widget_box.append(&entry);

    let response = gtk::TextView::new();
    response.set_editable(false);
    response.set_vexpand(true);
    response.set_wrap_mode(gtk4::WrapMode::Word);
    response.add_css_class("response");
    response.set_cursor_visible(false);

    let scrolled_response = ScrolledWindow::builder()
        .propagate_natural_height(true)
        .propagate_natural_width(true)
        .max_content_height(400)
        .max_content_width(600)
        .min_content_height(0)
        // .hscrollbar_policy(gtk::PolicyType::Automatic)
        // .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&response)
        .build();
    let response_revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .transition_duration(250)
        .reveal_child(false)
        .child(&scrolled_response)
        .build();

    let response_revealer_clone = response_revealer.clone();

    let current: Rc<RefCell<Option<glib::JoinHandle<()>>>> = Rc::new(RefCell::new(None));

    widget_box.append(&response_revealer);
    entry.connect_activate(glib::clone!(
        #[weak]
        response,
        move |entry| {
            if let Some(old) = current.borrow_mut().take() {
                old.abort(); // cancels at the task's next .await
                // Save the half-complete response to the history
                {
                    let mut history = chat_context.lock().unwrap();
                    let (start_iter, end_iter) = response.buffer().bounds();

                    history.push(ChatMessage::assistant(response.buffer().text(&start_iter, &end_iter, true).to_string()));
                }
            }
            let prompt = entry.text().to_string();
            entry.set_text("");
            entry.set_placeholder_text(Some(&prompt));

            let buffer = response.buffer();
            buffer.set_text("Thinking..."); // clear previous output
            let buffer = response.buffer();
            let ollama = Ollama::default();

            // Let's check if the request is a basic one or NEEDS an llm TODO!
            
            let url = format!("{}/v1/systemone", ollama.url_str().trim_end_matches('/'));

            let body = json!({
                "model": "tev1:0.8b",
                "state": prompt,
                "questions": {
                    "action": {
                        "type": "choice",
                        "instructions": "Which action does this request need?",
                        "criteria": {
                            "brightnessup":   "The user wants the screen brighter",
                            "brightnessdown": "The user wants the screen dimmer",
                            "volumeup":   "The user wants the volume higher",
                            "volumedown":   "The user wants the volume lower",
                            "close":   "The user wants to close this window",
                            "llmfast":  "A simple question answerable from basic knowledge, such as facts, definitions or short lookups",
                            "llmsmart": "A question that needs math, calculation, multi-step reasoning, code or analysis"
                        }
                    }
                }
            });

            let client = reqwest::Client::new();
            glib::spawn_future_local(async move {

                let resp: Value = 
                    client.post(url.clone())
                    .json(&body)
                    .send()
                    .await.expect("")
                    .error_for_status().expect("")
                    .json()
                    .await.expect("");

                let a = &resp["answers"]["action"];
                let choice = a["choice"].as_str().unwrap_or_default().to_string();
                let _ = a["confidence"].as_f64().unwrap_or(0.0);
                println!("{}", choice);
            });



            
            let model = "nemotron-3.5-bolt:e4b".to_string();

            response.add_css_class("responseExpander");
            entry.add_css_class("promptExpanded");
            response_revealer_clone.set_reveal_child(true);

            let context_clone = Arc::clone(&chat_context);

            let request = ChatMessageRequest::new(model.clone(), vec![ChatMessage::user(prompt)])
                .add_tool(get_weather);

            let handle = glib::spawn_future_local(async move {
                ollama_runner::render(ollama, buffer, context_clone, model.clone(), request).await;
            });
            *current.borrow_mut() = Some(handle);
        }
    ));

    win.set_child(Some(&widget_box));

    glib::timeout_add_local(
        std::time::Duration::from_millis(0),
        glib::clone!(
            #[weak]
            win,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move || {
                win.add_css_class("expanded");

                glib::ControlFlow::Break
            }
        ),
    );

    glib::timeout_add_local(
        std::time::Duration::from_millis(640),
        glib::clone!(
            #[weak]
            win,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move || {
                win.remove_css_class("expanded");
                win.add_css_class("locked");
                win.set_margin(Edge::Top, 250);

                glib::ControlFlow::Break
            }
        ),
    );

    // Escape closes
    let keys = gtk::EventControllerKey::new();
    let w = win.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::Escape {
            w.remove_css_class("locked");
            response_revealer.set_reveal_child(false);
            // w.set_margin(Edge::Top, 0);

            glib::timeout_add_local(
                std::time::Duration::from_millis(400),
                glib::clone!(
                    #[weak]
                    w,
                    #[upgrade_or]
                    glib::ControlFlow::Break,
                    move || {
                        w.close();
                    

                        glib::ControlFlow::Break
                    }
                ),
            );

            return gtk::glib::Propagation::Stop;
        }
        gtk::glib::Propagation::Proceed
    });
    win.add_controller(keys);

    win.present();
}
