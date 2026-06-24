use std::{io, time::Duration};

use axum::serve::Listener;
use futures::{
    future::BoxFuture,
    stream::{FuturesUnordered, StreamExt},
};
use tokio::net::{TcpListener, TcpStream};
use tracing::error;

pub struct MultiListener(Vec<TcpListener>);

impl From<Vec<TcpListener>> for MultiListener {
    fn from(value: Vec<TcpListener>) -> Self {
        MultiListener(value)
    }
}

impl Listener for MultiListener {
    type Io = TcpStream;
    type Addr = Vec<std::net::SocketAddr>;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        let mut tasks: FuturesUnordered<
            BoxFuture<
                '_,
                (
                    usize,
                    std::io::Result<(tokio::net::TcpStream, std::net::SocketAddr)>,
                ),
            >,
        > = FuturesUnordered::new();
        for (index, listener) in self.0.iter().enumerate() {
            tasks.push(Box::pin(async move { (index, listener.accept().await) }));
        }

        loop {
            match tasks.next().await {
                Some((index, Ok(tup))) => {
                    let listener = &self.0[index];
                    tasks.push(Box::pin(async move { (index, listener.accept().await) }));
                    return (tup.0, vec![tup.1]);
                }
                Some((index, Err(e))) => {
                    handle_accept_error(e).await;

                    let listener = &self.0[index];
                    tasks.push(Box::pin(async move { (index, listener.accept().await) }));
                }
                None => {
                    futures::future::pending::<()>().await;
                }
            }
        }
    }

    #[inline]
    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.0
            .iter()
            .map(|listener| listener.local_addr())
            .collect::<Result<Vec<_>, _>>()
    }
}

async fn handle_accept_error(e: io::Error) {
    if is_connection_error(&e) {
        return;
    }

    error!("accept error: {e}");
    tokio::time::sleep(Duration::from_secs(1)).await;
}

fn is_connection_error(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
    )
}
