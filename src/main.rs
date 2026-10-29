use std::{
    cell::RefCell, net::TcpStream, os::unix::process::CommandExt, process::Command, rc::Rc,
    time::Duration,
};

use futures::{FutureExt, StreamExt};
use gtk::prelude::*;
use gtk4::{self as gtk, ScrolledWindow, glib};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use ollama_rs::{
    Ollama,
    generation::completion::{GenerationContext, request::GenerationRequest},
};

const CSS: &str = "

window.copilot {
    background: transparent;
    min-width: 0px;
    
    transition: min-width 0.6s cubic-bezier(0.68, -0.6, 0.32, 1.6);
}

window.expanded {
    min-width: 500px;
}

window.copilot entry {

    padding: 12px 16px;
    font-size: 18px;
    border-radius: 12px 12px 12px 12px;
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
    let chat_context = Rc::new(RefCell::new(GenerationContext(vec![])));

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

    win.init_layer_shell();
    win.set_layer(Layer::Overlay);
    win.set_keyboard_mode(KeyboardMode::Exclusive);
    win.set_anchor(Edge::Top, true);
    win.set_margin(Edge::Top, 250);

    let widget_box = gtk::Box::new(gtk4::Orientation::Vertical, 0);

    let entry = gtk::Entry::builder().placeholder_text("").build();

    widget_box.append(&entry);

    let response = gtk::TextView::new();
    response.set_editable(false);
    response.set_vexpand(true);
    response.set_wrap_mode(gtk4::WrapMode::Word);
    response.add_css_class("response");

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

    widget_box.append(&response_revealer);
    entry.connect_activate(glib::clone!(
        #[weak]
        response,
        move |entry| {
            let prompt = entry.text().to_string();
            entry.set_text("");
            entry.set_placeholder_text(Some(&prompt));

            let buffer = response.buffer();
            buffer.set_text("Thinking..."); // clear previous output
            let buffer = response.buffer();
            let ollama = Ollama::default();
            let model = "nemotron-3.5-bolt:e4b".to_string();

            let x = GenerationContext(vec![]);
            let ctx = std::mem::replace(&mut *(chat_context.borrow_mut()), x);

            response.add_css_class("responseExpander");
            entry.add_css_class("promptExpanded");
            response_revealer.set_reveal_child(true);

            let request = GenerationRequest::new(model, prompt).context(ctx);

            glib::spawn_future_local(async move {
                match ollama.generate_stream(request.clone()).await {
                    Ok(mut stream) => {
                        let mut has_generated = false;
                        while let Some(chunk) = stream.next().await {
                            let responses = chunk.unwrap();
                            for resp in responses {
                                if !has_generated && resp.response.as_str() != "" {
                                    buffer.set_text("");
                                    has_generated = true;
                                }
                                buffer.insert(&mut buffer.end_iter(), resp.response.as_str());
                            }
                        }

                        request.context.expect("")
                    }
                    Err(e) => {
                        buffer.set_text(&format!("[error: {e}]"));
                        GenerationContext(vec![])
                    }
                }
            });

            // *chat_context.borrow_mut() = new_context;
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

    // Escape closes
    let keys = gtk::EventControllerKey::new();
    let w = win.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::Escape {
            w.close();
            return gtk::glib::Propagation::Stop;
        }
        gtk::glib::Propagation::Proceed
    });
    win.add_controller(keys);

    win.present();
}
