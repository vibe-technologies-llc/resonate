use std::{future::Future, net::SocketAddr, time::Duration};

use futures_util::{StreamExt, stream::FuturesUnordered};
use tokio::time::sleep;

use crate::{Error, Result};

pub const NEXT_ROUTE_AFTER: Duration = Duration::from_secs(1);

pub fn each_once(resolved: impl IntoIterator<Item = SocketAddr>) -> Vec<SocketAddr> {
    let mut routes = Vec::new();

    for route in resolved {
        if !routes.contains(&route) {
            routes.push(route);
        }
    }
    routes
}

pub async fn first_answering<T, F, A>(
    server: &str,
    routes: &[SocketAddr],
    attempt: A,
) -> Result<(T, SocketAddr)>
where
    A: Fn(SocketAddr) -> F,
    F: Future<Output = Result<T>>,
{
    let mut untried = routes.iter().copied();
    let mut trying = FuturesUnordered::new();
    let mut failed = None;

    loop {
        if trying.is_empty() {
            let Some(route) = untried.next() else {
                return Err(failed.unwrap_or_else(|| Error::Unresolved {
                    server: server.into(),
                }));
            };

            trying.push(tried(route, attempt(route)));
        }

        let more = untried.len() > 0;

        tokio::select! {
            Some((route, outcome)) = trying.next() => match outcome {
                Ok(answered) => return Ok((answered, route)),
                Err(error) => {
                    tracing::debug!(%error, %route, "a route to the sync server failed");
                    failed = Some(error);
                    if let Some(next) = untried.next() {
                        trying.push(tried(next, attempt(next)));
                    }
                }
            },
            () = sleep(NEXT_ROUTE_AFTER), if more => {
                if let Some(next) = untried.next() {
                    tracing::debug!(%next, "a route to the sync server is slow, so the next is tried beside it");
                    trying.push(tried(next, attempt(next)));
                }
            }
        }
    }
}

async fn tried<T>(
    route: SocketAddr,
    attempt: impl Future<Output = Result<T>>,
) -> (SocketAddr, Result<T>) {
    (route, attempt.await)
}

#[cfg(test)]
mod tests {
    use std::{
        future::pending,
        net::{IpAddr, Ipv4Addr},
    };

    use tokio::time::Instant;

    use super::*;

    const STALLED: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 443);
    const REFUSING: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 2)), 443);
    const WORKING: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 3)), 443);
    const SETTLES_WITHIN: Duration = Duration::from_millis(500);

    async fn answered_by(route: SocketAddr) -> Result<SocketAddr> {
        match route {
            WORKING => Ok(route),
            REFUSING => Err(Error::ServerSilent),
            _ => pending().await,
        }
    }

    #[tokio::test]
    async fn a_stalled_route_is_left_running_while_the_next_is_tried_and_the_first_to_answer_wins()
    {
        let started = Instant::now();

        let (answered, route) = first_answering("sync.example", &[STALLED, WORKING], answered_by)
            .await
            .expect("reached");
        let waited = started.elapsed();

        assert_eq!((answered, route), (WORKING, WORKING));
        assert!(waited >= NEXT_ROUTE_AFTER, "{waited:?}");
        assert!(waited < NEXT_ROUTE_AFTER + SETTLES_WITHIN, "{waited:?}");
    }

    #[tokio::test]
    async fn a_route_that_fails_hands_over_to_the_next_at_once() {
        let started = Instant::now();

        let reached = first_answering("sync.example", &[REFUSING, WORKING], answered_by).await;
        let waited = started.elapsed();

        assert_eq!(reached.expect("reached").1, WORKING);
        assert!(waited < SETTLES_WITHIN, "{waited:?}");
    }

    #[tokio::test]
    async fn every_route_failing_answers_the_last_failure_and_none_answers_unresolved() {
        let failed = first_answering("sync.example", &[REFUSING], answered_by).await;
        let none = first_answering("sync.example", &[], answered_by).await;

        assert!(matches!(failed, Err(Error::ServerSilent)));
        assert!(matches!(none, Err(Error::Unresolved { .. })));
    }

    #[test]
    fn a_route_resolved_twice_is_tried_once_in_the_order_first_resolved() {
        let routes = each_once([WORKING, STALLED, WORKING, REFUSING, STALLED]);

        assert_eq!(routes, [WORKING, STALLED, REFUSING]);
    }
}
