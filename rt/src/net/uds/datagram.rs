use std::io;
use std::marker::PhantomData;
use std::net::Shutdown;
use std::os::fd::{AsFd, IntoRawFd};

use a10::{AsyncFd, Extract};
use log::warn;
use socket2::{Domain, SockRef, Type};

use crate as rt;
use crate::io::{Buf, BufMut, BufWrapper};
use crate::net::uds::UnixAddr;
use crate::net::{Connected, Recv, Send, SendTo, Unconnected};

/// A Unix datagram socket.
#[derive(Debug)]
pub struct UnixDatagram<M = Unconnected> {
    fd: AsyncFd,
    /// The mode in which the socket is in, this determines what methods are
    /// available.
    mode: PhantomData<M>,
}

impl UnixDatagram {
    /// Creates a Unix datagram socket bound to `address`.
    pub async fn bind<RT>(rt: &RT, address: &UnixAddr) -> io::Result<UnixDatagram<Unconnected>>
    where
        RT: rt::Access,
    {
        let socket = UnixDatagram::unbound(rt).await?;
        socket.with_ref(|socket| socket.bind(&address.inner))?;
        Ok(socket)
    }

    /// Creates a Unix Datagram socket which is not bound to any address.
    pub async fn unbound<RT>(rt: &RT) -> io::Result<UnixDatagram<Unconnected>>
    where
        RT: rt::Access,
    {
        let fd = a10::net::socket(
            rt.submission_queue(),
            Domain::UNIX.into(),
            Type::DGRAM.cloexec().into(),
            0,
            0,
        )
        .await?;
        UnixDatagram::new(rt, fd)
    }

    /// Creates an unnamed pair of connected sockets.
    pub fn pair<RT>(rt: &RT) -> io::Result<(UnixDatagram<Connected>, UnixDatagram<Connected>)>
    where
        RT: rt::Access,
    {
        let (s1, s2) = socket2::Socket::pair(Domain::UNIX, Type::DGRAM.cloexec(), None)?;
        let s1 = UnixDatagram::new(rt, unsafe {
            // SAFETY: the call to `pair` above ensures the file descriptors are
            // valid.
            AsyncFd::new(s1.into_raw_fd(), rt.submission_queue())
        })?;
        let s2 = UnixDatagram::new(rt, unsafe {
            // SAFETY: Same as above.
            AsyncFd::new(s2.into_raw_fd(), rt.submission_queue())
        })?;
        Ok((s1, s2))
    }

    fn new<RT, M>(rt: &RT, fd: AsyncFd) -> io::Result<UnixDatagram<M>>
    where
        RT: rt::Access,
    {
        let socket = UnixDatagram {
            fd,
            mode: PhantomData,
        };

        #[cfg(target_os = "linux")]
        socket.with_ref(|socket| {
            if let Some(cpu) = rt.cpu() {
                if let Err(err) = socket.set_cpu_affinity(cpu) {
                    warn!("failed to set CPU affinity on UnixDatagram: {err}");
                }
            }
            Ok(())
        })?;

        Ok(socket)
    }
}

impl<M> UnixDatagram<M> {
    /// Connects the socket by setting the default destination and limiting
    /// packets that are received and send to the `remote` address.
    pub async fn connect(self, remote: UnixAddr) -> io::Result<UnixDatagram<Connected>> {
        self.fd.connect(remote).await?;
        Ok(UnixDatagram {
            fd: self.fd,
            mode: PhantomData,
        })
    }

    /// Returns the socket address of the remote peer of this socket.
    pub fn peer_addr(&mut self) -> io::Result<UnixAddr> {
        self.with_ref(|socket| socket.peer_addr().map(|a| UnixAddr { inner: a }))
    }

    /// Returns the socket address of the local half of this socket.
    pub fn local_addr(&mut self) -> io::Result<UnixAddr> {
        self.with_ref(|socket| socket.local_addr().map(|a| UnixAddr { inner: a }))
    }

    /// Shuts down the read, write, or both halves of this connection.
    ///
    /// This function will cause all pending and future I/O on the specified
    /// portions to return immediately with an appropriate value (see the
    /// documentation of [`Shutdown`]).
    pub fn shutdown(&mut self, how: Shutdown) -> io::Result<()> {
        self.with_ref(|socket| socket.shutdown(how))
    }

    /// Get the value of the `SO_ERROR` option on this socket.
    ///
    /// This will retrieve the stored error in the underlying socket, clearing
    /// the field in the process. This can be useful for checking errors between
    /// calls.
    pub fn take_error(&mut self) -> io::Result<Option<io::Error>> {
        self.with_ref(|socket| socket.take_error())
    }

    fn with_ref<F, T>(&self, f: F) -> io::Result<T>
    where
        F: FnOnce(SockRef<'_>) -> io::Result<T>,
    {
        let borrowed = self.fd.as_fd(); // TODO: remove this once we update to socket2 v0.5.
        f(SockRef::from(&borrowed))
    }
}

impl UnixDatagram<Unconnected> {
    // TODO: add `recv_from`, at the time of writing not supported in I/O uring.

    /// Send the bytes in `buf` to `address`.
    pub fn send_to<'a, B: Buf>(&'a mut self, buf: B, address: UnixAddr) -> SendTo<'a, B, UnixAddr> {
        SendTo(self.fd.sendto(BufWrapper(buf), address, 0).extract())
    }
}

impl UnixDatagram<Connected> {
    /// Recv bytes from the socket, writing them into `buf`.
    pub fn recv<'a, B: BufMut>(&'a mut self, buf: B) -> Recv<'a, B> {
        Recv(self.fd.recv(BufWrapper(buf), 0))
    }

    /// Send the bytes in `buf` to the socket's peer.
    pub fn send<'a, B: Buf>(&'a mut self, buf: B) -> Send<'a, B> {
        Send(self.fd.send(BufWrapper(buf), 0).extract())
    }
}