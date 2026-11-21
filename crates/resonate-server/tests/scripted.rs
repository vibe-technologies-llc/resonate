use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
};

use parking_lot::Mutex;
use resonate_sync::{
    accepted,
    wire::{FromServer, ToServer},
};
use tokio::{net::TcpListener, runtime::Runtime};

type Answering = dyn Fn(&ToServer) -> Vec<FromServer> + Send + Sync;

pub struct Scripted {
    pub address: SocketAddr,
    heard: Arc<Mutex<Vec<ToServer>>>,
    _runtime: Runtime,
}

impl Scripted {
    pub fn answering(
        answer: impl Fn(&ToServer) -> Vec<FromServer> + Send + Sync + 'static,
    ) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        let listener = runtime
            .block_on(TcpListener::bind(SocketAddr::from((
                Ipv4Addr::LOCALHOST,
                0,
            ))))
            .expect("listening");
        let address = listener.local_addr().expect("an address");
        let heard = Arc::new(Mutex::new(Vec::new()));
        let answer: Arc<Answering> = Arc::new(answer);

        let kept = Arc::clone(&heard);

        runtime.spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let heard = Arc::clone(&kept);
                let answer = Arc::clone(&answer);

                tokio::spawn(async move {
                    let Ok(mut session) = accepted(stream, None).await else {
                        return;
                    };
                    while let Ok(Some(frame)) = session.incoming.read().await {
                        let Ok(frame) = ToServer::decoded(&frame) else {
                            return;
                        };
                        let answers = answer(&frame);

                        heard.lock().push(frame);
                        for answered in answers {
                            if session.outgoing.send(&answered.encoded()).await.is_err() {
                                return;
                            }
                        }
                    }
                });
            }
        });

        Self {
            address,
            heard,
            _runtime: runtime,
        }
    }

    pub fn heard(&self) -> Vec<ToServer> {
        self.heard.lock().clone()
    }
}
