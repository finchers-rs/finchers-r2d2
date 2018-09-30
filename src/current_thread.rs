//! The implementation of PoolEndpoint which works on the current thread.

pub use self::imp::{pool_endpoint, PoolEndpoint};

mod imp {
    use finchers::endpoint::{ApplyContext, ApplyResult, Endpoint};
    use finchers::error;
    use finchers::error::Error;

    use futures::{Async, Future, Poll};
    use r2d2::{ManageConnection, Pool, PooledConnection};
    use std::fmt;

    /// Create an endpoint which acquires the connection from the specified connection pool.
    ///
    /// This endpoint will block the current thread during retrieving the connection from pool.
    pub fn pool_endpoint<M>(pool: Pool<M>) -> PoolEndpoint<M>
    where
        M: ManageConnection,
    {
        PoolEndpoint { pool }
    }

    /// The endpoint which retrieves a connection from a connection pool.
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

        fn apply(&'a self, _: &mut ApplyContext<'_>) -> ApplyResult<Self::Future> {
            Ok(PoolFuture { endpoint: self })
        }
    }

    // not a public API.
    pub struct PoolFuture<'a, M: ManageConnection> {
        endpoint: &'a PoolEndpoint<M>,
    }

    impl<'a, M> fmt::Debug for PoolFuture<'a, M>
    where
        M: ManageConnection + fmt::Debug,
        M::Connection: fmt::Debug,
    {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("PoolFuture")
                .field("endpoint", &self.endpoint)
                .finish()
        }
    }

    impl<'a, M> Future for PoolFuture<'a, M>
    where
        M: ManageConnection,
    {
        type Item = (PooledConnection<M>,);
        type Error = Error;

        fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
            self.endpoint
                .pool
                .get()
                .map(|conn| Async::Ready((conn,)))
                .map_err(error::fail)
        }
    }
}
