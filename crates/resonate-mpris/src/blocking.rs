#![allow(dead_code)]

use std::{
    future::{Future, poll_fn},
    pin::{Pin, pin},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    thread::{self, Thread},
};

struct Unparker {
    thread: Thread,
    woken: AtomicBool,
}

impl Wake for Unparker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.woken.store(true, Ordering::Release);
        self.thread.unpark();
    }
}

pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    let unparker = Arc::new(Unparker {
        thread: thread::current(),
        woken: AtomicBool::new(false),
    });
    let waker = Waker::from(Arc::clone(&unparker));
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);

    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        while !unparker.woken.swap(false, Ordering::Acquire) {
            thread::park();
        }
    }
}
use zbus::{
    DynamicType, Message, ObjectPath, Type,
    export::futures_core::Stream,
    fdo::{RequestNameFlags, RequestNameReply},
    names::{BusName, InterfaceName, MemberName, OwnedBusName, WellKnownName},
    object_server::{Interface, InterfaceDeref},
    wire::DynamicDeserialize,
};

#[derive(Clone)]
pub(crate) struct Connection(zbus::Connection);

impl Connection {
    pub(crate) fn session() -> zbus::Result<Self> {
        block_on(zbus::Connection::session()).map(Self)
    }

    pub(crate) fn call_method<'d, 'p, 'i, 'm, D, P, I, M, B>(
        &self,
        destination: Option<D>,
        path: P,
        interface: Option<I>,
        method_name: M,
        body: &B,
    ) -> zbus::Result<Message>
    where
        D: TryInto<BusName<'d>>,
        P: TryInto<ObjectPath<'p>>,
        I: TryInto<InterfaceName<'i>>,
        M: TryInto<MemberName<'m>>,
        D::Error: Into<zbus::Error>,
        P::Error: Into<zbus::Error>,
        I::Error: Into<zbus::Error>,
        M::Error: Into<zbus::Error>,
        B: zbus::export::serde::Serialize + DynamicType,
    {
        block_on(
            self.0
                .call_method(destination, path, interface, method_name, body),
        )
    }

    pub(crate) fn emit_signal<'d, 'p, 'i, 'm, D, P, I, M, B>(
        &self,
        destination: Option<D>,
        path: P,
        interface: I,
        signal_name: M,
        body: &B,
    ) -> zbus::Result<()>
    where
        D: TryInto<BusName<'d>>,
        P: TryInto<ObjectPath<'p>>,
        I: TryInto<InterfaceName<'i>>,
        M: TryInto<MemberName<'m>>,
        D::Error: Into<zbus::Error>,
        P::Error: Into<zbus::Error>,
        I::Error: Into<zbus::Error>,
        M::Error: Into<zbus::Error>,
        B: zbus::export::serde::Serialize + DynamicType,
    {
        block_on(
            self.0
                .emit_signal(destination, path, interface, signal_name, body),
        )
    }

    pub(crate) fn request_name_with_flags<'w, W>(
        &self,
        name: W,
        flags: RequestNameFlags,
    ) -> zbus::Result<RequestNameReply>
    where
        W: TryInto<WellKnownName<'w>>,
        W::Error: Into<zbus::Error>,
    {
        block_on(self.0.request_name_with_flags(name, flags))
    }

    pub(crate) fn release_name<'w, W>(&self, name: W) -> zbus::Result<bool>
    where
        W: TryInto<WellKnownName<'w>>,
        W::Error: Into<zbus::Error>,
    {
        block_on(self.0.release_name(name))
    }

    pub(crate) fn close(&self) -> zbus::Result<()> {
        block_on(self.0.clone().close())
    }

    pub(crate) fn unique_name(&self) -> Option<&zbus::names::OwnedUniqueName> {
        self.0.unique_name()
    }

    pub(crate) fn object_server(&self) -> ObjectServer {
        ObjectServer(self.0.clone())
    }
}

pub(crate) struct ObjectServer(zbus::Connection);

impl ObjectServer {
    pub(crate) fn at<'p, P, I>(&self, path: P, iface: I) -> zbus::Result<Option<I>>
    where
        I: Interface,
        P: TryInto<ObjectPath<'p>>,
        P::Error: Into<zbus::Error>,
    {
        block_on(self.0.object_server().at(path, iface))
    }

    pub(crate) fn interface<'p, P, I>(&self, path: P) -> zbus::Result<InterfaceRef<I>>
    where
        I: Interface,
        P: TryInto<ObjectPath<'p>>,
        P::Error: Into<zbus::Error>,
    {
        block_on(self.0.object_server().interface(path)).map(InterfaceRef)
    }
}

pub(crate) struct InterfaceRef<I>(zbus::object_server::InterfaceRef<I>);

impl<I: 'static> InterfaceRef<I> {
    pub(crate) fn get(&self) -> InterfaceDeref<'_, I> {
        block_on(self.0.get())
    }

    pub(crate) fn signal_emitter(&self) -> &zbus::object_server::SignalEmitter<'static> {
        self.0.signal_emitter()
    }
}

pub(crate) mod connection {
    use std::time::Duration;

    use zbus::{ObjectPath, names::WellKnownName, object_server::Interface};

    use super::{Connection, block_on};

    pub(crate) struct Builder(zbus::connection::Builder<'static>);

    impl Builder {
        pub(crate) fn session() -> zbus::Result<Self> {
            Ok(Self(zbus::connection::Builder::session()))
        }

        pub(crate) fn system() -> zbus::Result<Self> {
            Ok(Self(zbus::connection::Builder::system()))
        }

        pub(crate) fn method_timeout(self, timeout: Duration) -> Self {
            Self(self.0.method_timeout(timeout))
        }

        pub(crate) fn serve_at<P, I>(self, path: P, iface: I) -> zbus::Result<Self>
        where
            I: Interface,
            P: TryInto<ObjectPath<'static>>,
            P::Error: Into<zbus::Error>,
        {
            Ok(Self(self.0.serve_at(path, iface)))
        }
        pub(crate) fn name<W>(self, name: W) -> zbus::Result<Self>
        where
            W: TryInto<WellKnownName<'static>>,
            W::Error: Into<zbus::Error>,
        {
            Ok(Self(self.0.name(name)))
        }

        pub(crate) fn build(self) -> zbus::Result<Connection> {
            block_on(self.0.build()).map(Connection)
        }
    }
}

#[derive(Clone)]
pub(crate) struct Proxy<'a> {
    inner: zbus::Proxy<'a>,
    connection: Connection,
}

impl<'a> Proxy<'a> {
    pub(crate) fn new<D, P, I>(
        connection: &Connection,
        destination: D,
        path: P,
        interface: I,
    ) -> zbus::Result<Self>
    where
        D: TryInto<BusName<'a>>,
        P: TryInto<ObjectPath<'a>>,
        I: TryInto<InterfaceName<'a>>,
        D::Error: Into<zbus::Error>,
        P::Error: Into<zbus::Error>,
        I::Error: Into<zbus::Error>,
    {
        block_on(zbus::Proxy::new(
            &connection.0,
            destination,
            path,
            interface,
        ))
        .map(|inner| Self {
            inner,
            connection: connection.clone(),
        })
    }

    pub(crate) const fn connection(&self) -> &Connection {
        &self.connection
    }

    pub(crate) fn call<'m, M, B, R>(&self, method_name: M, body: &B) -> zbus::Result<R>
    where
        M: TryInto<MemberName<'m>>,
        M::Error: Into<zbus::Error>,
        B: zbus::export::serde::Serialize + DynamicType,
        R: for<'d> DynamicDeserialize<'d>,
    {
        block_on(self.inner.call(method_name, body))
    }

    pub(crate) fn call_method<'m, M, B>(&self, method_name: M, body: &B) -> zbus::Result<Message>
    where
        M: TryInto<MemberName<'m>>,
        M::Error: Into<zbus::Error>,
        B: zbus::export::serde::Serialize + DynamicType,
    {
        block_on(self.inner.call_method(method_name, body))
    }

    pub(crate) fn get_property<T>(&self, property_name: &str) -> zbus::Result<T>
    where
        T: zbus::export::serde::de::DeserializeOwned + Type,
    {
        block_on(self.inner.get_property(property_name))
    }

    pub(crate) fn set_property<T>(&self, property_name: &str, value: T) -> zbus::fdo::Result<()>
    where
        T: zbus::export::serde::Serialize + Type,
    {
        block_on(self.inner.set_property(property_name, value))
    }
    pub(crate) fn receive_all_signals(
        &self,
    ) -> zbus::Result<Arrivals<zbus::proxy::SignalStream<'static>>> {
        block_on(self.inner.receive_all_signals()).map(Arrivals)
    }

    pub(crate) fn receive_signal<'m, M>(
        &self,
        signal_name: M,
    ) -> zbus::Result<Arrivals<zbus::proxy::SignalStream<'m>>>
    where
        M: TryInto<MemberName<'m>>,
        M::Error: Into<zbus::Error>,
    {
        block_on(self.inner.receive_signal(signal_name)).map(Arrivals)
    }
}

pub(crate) struct Arrivals<S>(S);

impl<S: Stream + Unpin> Iterator for Arrivals<S> {
    type Item = S::Item;

    fn next(&mut self) -> Option<S::Item> {
        block_on(poll_fn(|context| Pin::new(&mut self.0).poll_next(context)))
    }
}

pub(crate) mod fdo {
    use zbus::{
        fdo::{InterfacesAddedStream, ManagedObjects, NameLostStream},
        names::BusName,
        proxy::CacheProperties,
    };

    use super::{Arrivals, Connection, OwnedBusName, block_on};

    pub(crate) struct DBusProxy(zbus::fdo::DBusProxy<'static>);

    impl DBusProxy {
        pub(crate) fn new(connection: &Connection) -> zbus::Result<Self> {
            block_on(zbus::fdo::DBusProxy::new(&connection.0)).map(Self)
        }

        pub(crate) fn receive_name_lost(&self) -> zbus::Result<Arrivals<NameLostStream>> {
            block_on(self.0.receive_name_lost()).map(Arrivals)
        }
        pub(crate) fn get_name_owner(
            &self,
            name: BusName<'_>,
        ) -> zbus::fdo::Result<zbus::names::OwnedUniqueName> {
            block_on(self.0.get_name_owner(name))
        }

        pub(crate) fn list_names(&self) -> zbus::fdo::Result<Vec<OwnedBusName>> {
            block_on(self.0.list_names())
        }

        pub(crate) fn name_has_owner(&self, name: BusName<'_>) -> zbus::fdo::Result<bool> {
            block_on(self.0.name_has_owner(name))
        }
    }

    pub(crate) struct ObjectManagerProxy(zbus::fdo::ObjectManagerProxy<'static>);

    impl ObjectManagerProxy {
        pub(crate) fn at(
            connection: &Connection,
            destination: &'static str,
            path: &'static str,
        ) -> zbus::Result<Self> {
            block_on(
                zbus::fdo::ObjectManagerProxy::builder(&connection.0)
                    .destination(destination)
                    .path(path)
                    .cache_properties(CacheProperties::No)
                    .build(),
            )
            .map(Self)
        }

        pub(crate) fn receive_interfaces_added(
            &self,
        ) -> zbus::Result<Arrivals<InterfacesAddedStream>> {
            block_on(self.0.receive_interfaces_added()).map(Arrivals)
        }

        pub(crate) fn get_managed_objects(&self) -> zbus::fdo::Result<ManagedObjects> {
            block_on(self.0.get_managed_objects())
        }
    }
}
