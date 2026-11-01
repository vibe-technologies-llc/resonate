use std::sync::{Arc, atomic::AtomicBool};

use crossbeam_channel::Receiver;
use windows::{
    Data::Xml::Dom::XmlDocument,
    Foundation::TypedEventHandler,
    UI::Notifications::{
        ToastActivatedEventArgs, ToastNotification, ToastNotificationManager, ToastNotifier,
    },
    core::{HSTRING, IInspectable, Interface},
};

use crate::{
    Error, Result, WindowsOp,
    apartment::Apartment,
    desktop::Errand,
    identity::APP_USER_MODEL_ID,
    interfaces::Shared,
    notify::{Shown, pressed},
    toast::{Buttons, document},
};

const GROUP: &str = "resonate";
const TRACK_TAG: &str = "track";
const LISTEN_TAG: &str = "listen";

pub(crate) struct Desk;

impl Desk {
    pub(crate) const fn open() -> Result<Self> {
        Ok(Self)
    }

    pub(crate) fn run(
        self,
        shared: &Arc<Shared>,
        errands: &Receiver<Errand>,
        _wanted: &AtomicBool,
    ) {
        let _apartment = match Apartment::enter() {
            Ok(apartment) => apartment,
            Err(error) => {
                tracing::warn!(%error, "a track change will not be told to Windows");
                return;
            }
        };
        let mut toaster = match Toaster::new(shared) {
            Ok(toaster) => toaster,
            Err(error) => {
                tracing::warn!(%error, "a track change will not be told to Windows");
                return;
            }
        };

        for errand in errands {
            match errand {
                Errand::Show(shown) => toaster.show(&shown),
                Errand::Tell(shown) => toaster.tell(&shown),
                Errand::Grip => {}
            }
        }

        toaster.hush();
    }
}

struct Toaster {
    notifier: ToastNotifier,
    shared: Arc<Shared>,
    raised: Option<ToastNotification>,
}

impl Toaster {
    fn new(shared: &Arc<Shared>) -> Result<Self> {
        let notifier =
            ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_USER_MODEL_ID))
                .map_err(|source| Error::windows(WindowsOp::Notifier, source))?;

        Ok(Self {
            notifier,
            shared: Arc::clone(shared),
            raised: None,
        })
    }

    fn show(&mut self, shown: &Shown) {
        match self.raise(shown, Buttons::Offered, TRACK_TAG) {
            Ok(toast) => self.raised = Some(toast),
            Err(error) => {
                tracing::debug!(%error, "a track change did not reach Windows");
            }
        }
    }

    fn tell(&self, shown: &Shown) {
        if let Err(error) = self.raise(shown, Buttons::Withheld, LISTEN_TAG) {
            tracing::debug!(%error, "what Listen named did not reach Windows");
        }
    }

    fn raise(&self, shown: &Shown, buttons: Buttons, tag: &str) -> Result<ToastNotification> {
        let template =
            XmlDocument::new().map_err(|source| Error::windows(WindowsOp::Template, source))?;
        template
            .LoadXml(&HSTRING::from(document(shown, buttons)))
            .map_err(|source| Error::windows(WindowsOp::Template, source))?;

        let toast = ToastNotification::CreateToastNotification(&template)
            .map_err(|source| Error::windows(WindowsOp::Toast, source))?;
        toast
            .SetTag(&HSTRING::from(tag))
            .map_err(|source| Error::windows(WindowsOp::Toast, source))?;
        toast
            .SetGroup(&HSTRING::from(GROUP))
            .map_err(|source| Error::windows(WindowsOp::Toast, source))?;

        if buttons == Buttons::Offered {
            self.answer_presses(&toast)?;
        }
        self.notifier
            .Show(&toast)
            .map_err(|source| Error::windows(WindowsOp::Show, source))?;

        Ok(toast)
    }

    fn answer_presses(&self, toast: &ToastNotification) -> Result<()> {
        let shared = Arc::clone(&self.shared);
        let handler =
            TypedEventHandler::<ToastNotification, IInspectable>::new(move |_, arguments| {
                let command = arguments
                    .as_ref()
                    .and_then(key_pressed)
                    .and_then(|key| pressed(&key));
                if let Some(command) = command {
                    let _ = shared.settle(command);
                }
                Ok(())
            });

        toast
            .Activated(&handler)
            .map(drop)
            .map_err(|source| Error::windows(WindowsOp::Listen, source))
    }

    fn hush(&mut self) {
        let Some(toast) = self.raised.take() else {
            return;
        };
        if let Err(source) = self.notifier.Hide(&toast) {
            let error = Error::windows(WindowsOp::Hide, source);
            tracing::debug!(%error, "a notification outlived the player that raised it");
        }
    }
}

fn key_pressed(arguments: &IInspectable) -> Option<String> {
    arguments
        .cast::<ToastActivatedEventArgs>()
        .ok()?
        .Arguments()
        .ok()
        .map(|key| key.to_string())
}
