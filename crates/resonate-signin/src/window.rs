use std::{
    convert::Infallible,
    path::PathBuf,
    time::{Duration, Instant},
};

use tao::{
    dpi::LogicalSize,
    event::{Event, StartCause, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    window::WindowBuilder,
};
use wry::{PageLoadEvent, WebContext, WebView, WebViewBuilder};

use crate::{
    CLOSED, Cookie, EngineOp, Error, FAILED, Result, SIGN_IN_PAGE, SIGNED_IN, is_youtube_music,
    kept, signed_in, written,
};

const TITLE: &str = "Sign in to YouTube Music";

const LOOKED_AT_EVERY: Duration = Duration::from_secs(1);

const WIDTH: f64 = 480.0;

const HEIGHT: f64 = 720.0;

#[derive(Clone, Copy, Debug)]
enum Woken {
    Loaded,
}

enum Looked {
    Waiting,
    Kept,
}

pub fn sign_in(jar: PathBuf, profile: PathBuf) -> Result<Infallible> {
    let event_loop = EventLoopBuilder::<Woken>::with_user_event().build();
    let window = WindowBuilder::new()
        .with_title(TITLE)
        .with_inner_size(LogicalSize::new(WIDTH, HEIGHT))
        .build(&event_loop)
        .map_err(|source| Error::Window { source })?;
    let mut context = WebContext::new(Some(profile));
    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_url(SIGN_IN_PAGE)
        .with_on_page_load_handler(told_when_loaded(event_loop.create_proxy()));
    let webview = built(builder, &window)?;

    event_loop.run(move |event, _, flow| {
        let _held = (&window, &context);
        match event {
            Event::NewEvents(StartCause::Init) => *flow = looked_at_again(),
            Event::NewEvents(StartCause::ResumeTimeReached { .. })
            | Event::UserEvent(Woken::Loaded) => {
                *flow = match looked(&webview, &jar) {
                    Ok(Looked::Waiting) => looked_at_again(),
                    Ok(Looked::Kept) => ControlFlow::ExitWithCode(SIGNED_IN),
                    Err(error) => {
                        tracing::error!(%error, "the sign-in could not be kept");
                        ControlFlow::ExitWithCode(FAILED)
                    }
                };
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *flow = ControlFlow::ExitWithCode(CLOSED),
            _ => {}
        }
    })
}

fn told_when_loaded(proxy: EventLoopProxy<Woken>) -> impl Fn(PageLoadEvent, String) + 'static {
    move |loaded, _| {
        if matches!(loaded, PageLoadEvent::Finished) {
            let _ = proxy.send_event(Woken::Loaded);
        }
    }
}

fn looked_at_again() -> ControlFlow {
    ControlFlow::WaitUntil(Instant::now() + LOOKED_AT_EVERY)
}

#[cfg(target_os = "linux")]
fn built(builder: WebViewBuilder<'_>, window: &tao::window::Window) -> Result<WebView> {
    use tao::platform::unix::WindowExtUnix as _;
    use wry::WebViewBuilderExtUnix as _;

    let Some(container) = window.default_vbox() else {
        return Err(Error::NoContainer);
    };
    builder
        .build_gtk(container)
        .map_err(|source| Error::Engine {
            op: EngineOp::Build,
            source: Box::new(source),
        })
}

#[cfg(not(target_os = "linux"))]
fn built(builder: WebViewBuilder<'_>, window: &tao::window::Window) -> Result<WebView> {
    builder.build(window).map_err(|source| Error::Engine {
        op: EngineOp::Build,
        source: Box::new(source),
    })
}

fn looked(webview: &WebView, jar: &std::path::Path) -> Result<Looked> {
    let address = webview.url().map_err(|source| Error::Engine {
        op: EngineOp::Address,
        source: Box::new(source),
    })?;
    if !is_youtube_music(&address) {
        return Ok(Looked::Waiting);
    }
    let held = webview.cookies().map_err(|source| Error::Engine {
        op: EngineOp::Cookies,
        source: Box::new(source),
    })?;
    let cookies = kept(held.iter().filter_map(cookie_of).collect());
    if !signed_in(&cookies) {
        return Ok(Looked::Waiting);
    }
    written(jar, &cookies)?;
    Ok(Looked::Kept)
}

fn cookie_of(held: &wry::cookie::Cookie<'static>) -> Option<Cookie> {
    Some(Cookie {
        domain: held.domain()?.to_owned(),
        path: held.path().unwrap_or("/").to_owned(),
        secure: held.secure().unwrap_or(false),
        http_only: held.http_only().unwrap_or(false),
        expires: held
            .expires_datetime()
            .and_then(|at| u64::try_from(at.unix_timestamp()).ok()),
        name: held.name().to_owned(),
        value: held.value().to_owned(),
    })
}
