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

pub use impl_endpoint::{pool_endpoint, PoolEndpoint};
#[doc(no_inline)]
pub use r2d2::*;

mod impl_endpoint {
    use finchers::endpoint::{Context, Endpoint, EndpointResult};
    use finchers::error::Error;

    use futures::{task, Async, Future, Poll};
    use r2d2::{ManageConnection, Pool, PooledConnection};

    /// Create an endpoint which acquires the connection from the specified connection pool.
    ///
    /// This endpoint internally calls the blocking section `Pool::get()` in the current thread
    /// and hence will block the current thread.
    pub fn pool_endpoint<M>(pool: Pool<M>) -> PoolEndpoint<M>
    where
        M: ManageConnection,
    {
        PoolEndpoint { pool }
    }

    #[allow(missing_docs)]
    #[derive(Debug, Clone)]
    pub struct PoolEndpoint<M: ManageConnection> {
        pool: Pool<M>,
    }

    impl<'a, M> Endpoint<'a> for PoolEndpoint<M>
    where
        M: ManageConnection + 'a,
    {
        type Output = (PooledConnection<M>,);
        type Future = PoolFuture<'a, M>;

        fn apply(&'a self, _: &mut Context<'_>) -> EndpointResult<Self::Future> {
            Ok(PoolFuture { endpoint: self })
        }
    }

    #[derive(Debug)]
    pub struct PoolFuture<'a, M: ManageConnection> {
        endpoint: &'a PoolEndpoint<M>,
    }

    impl<'a, M> Future for PoolFuture<'a, M>
    where
        M: ManageConnection,
    {
        type Item = (PooledConnection<M>,);
        type Error = Error;

        fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
            trace!("calling pool.try_get()");
            match self.endpoint.pool.try_get() {
                Some(conn) => {
                    trace!("--> success to retrieve a connection");
                    Ok(Async::Ready((conn,)))
                }
                None => {
                    trace!("--> no idle connections available");
                    let task = task::current();
                    task.notify();
                    Ok(Async::NotReady)
                }
            }
        }
    }
}
