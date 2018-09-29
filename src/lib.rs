//! Connection pool support for Finchers

// master
#![doc(html_root_url = "https://finchers-rs.github.io/finchers-r2d2")]
// released
//#![doc(html_root_url = "https://docs.rs/finchers-r2d2/0.1.0")]
#![warn(
    missing_docs,
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    unused,
)]
//#![warn(rust_2018_compatibility)]
#![cfg_attr(feature = "strict", deny(warnings))]
#![cfg_attr(feature = "strict", doc(test(attr(deny(warnings)))))]

extern crate finchers;
extern crate futures;
#[macro_use]
extern crate log;
extern crate r2d2;
extern crate tokio_executor;
extern crate tokio_threadpool;

pub use impl_endpoint::{pool_endpoint, PoolEndpoint};
#[doc(no_inline)]
pub use r2d2::*;

mod impl_endpoint {
    use finchers::endpoint::{Context, Endpoint, EndpointResult};
    use finchers::error;
    use finchers::error::Error;

    use futures::future::poll_fn;
    use futures::sync::oneshot;
    use futures::{Async, Future, Poll};
    use r2d2::{ManageConnection, Pool, PooledConnection};
    use std::fmt;
    use tokio_executor::{DefaultExecutor, Executor};
    use tokio_threadpool::blocking;

    /// Create an endpoint which acquires the connection from the specified connection pool.
    ///
    /// This endpoint internally spawns the task for executing the blocking section,
    /// by using the specified "Executor".
    pub fn pool_endpoint<M>(pool: Pool<M>) -> PoolEndpoint<M, impl Fn() -> DefaultExecutor>
    where
        M: ManageConnection,
    {
        PoolEndpoint {
            pool,
            spawner_fn: || DefaultExecutor::current(),
            with_blocking_api: true,
        }
    }

    #[allow(missing_docs)]
    #[derive(Debug, Clone)]
    pub struct PoolEndpoint<M: ManageConnection, F> {
        pool: Pool<M>,
        spawner_fn: F,
        with_blocking_api: bool,
    }

    impl<M, Sp> PoolEndpoint<M, Sp>
    where
        M: ManageConnection,
        Sp: Executor,
    {
        /// Sets the function which generates an "Executor" which spawns the task
        /// to execute the blocking section.
        pub fn with_spawner_fn<F, T>(self, spawner_fn: F) -> PoolEndpoint<M, F>
        where
            F: Fn() -> T,
            T: Executor,
        {
            PoolEndpoint {
                pool: self.pool,
                spawner_fn,
                with_blocking_api: false,
            }
        }
    }

    impl<'a, M, F, T> Endpoint<'a> for PoolEndpoint<M, F>
    where
        M: ManageConnection + 'a,
        F: Fn() -> T + 'a,
        T: Executor,
    {
        type Output = (PooledConnection<M>,);
        type Future = PoolFuture<'a, M, F>;

        fn apply(&'a self, _: &mut Context<'_>) -> EndpointResult<Self::Future> {
            Ok(PoolFuture {
                endpoint: self,
                rx: None,
            })
        }
    }

    pub struct PoolFuture<'a, M: ManageConnection, F: 'a> {
        endpoint: &'a PoolEndpoint<M, F>,
        rx: Option<oneshot::Receiver<Result<PooledConnection<M>, Error>>>,
    }

    impl<'a, M, F> fmt::Debug for PoolFuture<'a, M, F>
    where
        M: ManageConnection + fmt::Debug,
        M::Connection: fmt::Debug,
        F: fmt::Debug,
    {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("PoolFuture")
                .field("endpoint", &self.endpoint)
                .field("rx", &self.rx)
                .finish()
        }
    }

    impl<'a, M, F, T> Future for PoolFuture<'a, M, F>
    where
        M: ManageConnection,
        F: Fn() -> T,
        T: Executor,
    {
        type Item = (PooledConnection<M>,);
        type Error = Error;

        fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
            loop {
                if let Some(ref mut rx) = self.rx {
                    trace!("retrieved the connection from spawned task");
                    return match rx.poll() {
                        Ok(Async::NotReady) => Ok(Async::NotReady),
                        Ok(Async::Ready(res)) => res.map(|conn| Async::Ready((conn,))),
                        Err(_canceled) => panic!(),
                    };
                }

                if let Some(conn) = self.endpoint.pool.try_get() {
                    trace!("retrieved the connection without spawning the task");
                    return Ok(Async::Ready((conn,)));
                }

                trace!("spawning the task for executing blocking section");
                let rx = {
                    let pool = self.endpoint.pool.clone();
                    let (tx, rx) = oneshot::channel();
                    let with_blocking_api = self.endpoint.with_blocking_api;
                    let mut tx_opt = Some(tx);
                    let future = poll_fn(move || {
                        let result = if with_blocking_api {
                            match blocking(|| pool.get()) {
                                Ok(Async::NotReady) => return Ok(Async::NotReady),
                                Ok(Async::Ready(res)) => res.map_err(error::fail),
                                Err(blocking_err) => Err(error::fail(blocking_err)),
                            }
                        } else {
                            pool.get().map_err(error::fail)
                        };
                        tx_opt
                            .take()
                            .expect("the sender has already taken")
                            .send(result)
                            .unwrap_or_else(|_| panic!("failed to send the result"));
                        Ok(Async::Ready(()))
                    });

                    let mut spawner = (self.endpoint.spawner_fn)();
                    spawner.spawn(Box::new(future)).map_err(error::fail)?;

                    rx
                };

                self.rx = Some(rx);
            }
        }
    }
}
