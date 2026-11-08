//! Mesure locale du dispatch clavier et du rendu, sans API ni session graphique.
//! cargo run -p hyprpanel-chat --example typing_perf --offline --locked

#[path = "../src/avatar.rs"]
mod avatar;
#[path = "../src/i18n.rs"]
mod i18n;

use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, ModelRc, PhysicalSize, SharedPixelBuffer, VecModel};
use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

slint::include_modules!();

struct PerfPlatform {
    window: Rc<MinimalSoftwareWindow>,
    clock: Rc<Cell<Duration>>,
}
impl Platform for PerfPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }
    fn duration_since_start(&self) -> Duration {
        self.clock.get()
    }
}

fn main() -> anyhow::Result<()> {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
    let clock = Rc::new(Cell::new(Duration::ZERO));
    slint::platform::set_platform(Box::new(PerfPlatform {
        window: window.clone(),
        clock: clock.clone(),
    }))?;
    let app = ChatWindow::new()?;
    avatar::install(&app);
    app.set_tr(i18n::tr(i18n::Lang::Fr).to_slint());
    app.set_messages(ModelRc::new(VecModel::<MessageRow>::default()));
    app.set_attachments(ModelRc::new(VecModel::<AttachmentRow>::default()));
    app.set_history(ModelRc::new(VecModel::<HistoryRow>::default()));
    app.set_model_label("GLM-5.3".into());
    app.show()?;
    window.set_size(PhysicalSize::new(1080, 800));
    app.invoke_focus_composer();
    let mut pixels = SharedPixelBuffer::<slint::Rgb8Pixel>::new(1080, 800);

    for animated in [false, true] {
        app.set_animations_enabled(animated);
        app.set_draft("".into());
        let mut idle = Vec::new();
        let mut input = Vec::new();
        let mut paints = 0;
        let mut typing_phase = None;
        let idle_phase = app.invoke_animation_time();
        let expected = "Je voudrais un bureau plus lisible, avec une palette douce. ".repeat(2);
        for (index, key) in std::iter::repeat(None)
            .take(90)
            .chain(expected.chars().map(Some))
            .enumerate()
        {
            clock.set(clock.get() + Duration::from_millis(16));
            let started = Instant::now();
            if let Some(key) = key {
                app.window().dispatch_event(WindowEvent::KeyPressed {
                    text: key.to_string().into(),
                });
                app.window().dispatch_event(WindowEvent::KeyReleased {
                    text: key.to_string().into(),
                });
            }
            slint::platform::update_timers_and_animations();
            if window.draw_if_needed(|renderer| {
                renderer.render(pixels.make_mut_slice(), 1080);
            }) {
                paints += 1;
            }
            if key.is_some() {
                let phase = app.invoke_animation_time();
                assert_eq!(
                    *typing_phase.get_or_insert(phase),
                    phase,
                    "l'animation doit rester en pause pendant la frappe"
                );
            }
            let elapsed = started.elapsed().as_secs_f64() * 1000.;
            if index > 15 {
                if key.is_some() {
                    input.push(elapsed);
                } else {
                    idle.push(elapsed);
                }
            }
        }
        assert_eq!(app.get_draft().as_str(), expected);
        if animated {
            assert!(
                typing_phase.unwrap() > idle_phase,
                "l'avatar doit bouger au repos"
            );
        }
        idle.sort_by(f64::total_cmp);
        input.sort_by(f64::total_cmp);
        println!(
            "animated={animated} idle_mean_ms={:.2} typing_p50_ms={:.2} typing_p95_ms={:.2} paints={paints}",
            idle.iter().sum::<f64>() / idle.len() as f64,
            input[input.len() / 2],
            input[input.len() * 95 / 100]
        );
        for _ in 0..30 {
            clock.set(clock.get() + Duration::from_millis(16));
            slint::platform::update_timers_and_animations();
            window.draw_if_needed(|renderer| {
                renderer.render(pixels.make_mut_slice(), 1080);
            });
        }
        if animated {
            assert!(
                app.invoke_animation_time() > typing_phase.unwrap(),
                "l'animation doit reprendre après la saisie"
            );
        }
    }
    Ok(())
}
