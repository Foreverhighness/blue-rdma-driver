use std::{
    io,
    ops::{Deref, DerefMut},
};

use crate::desc::simple_nic::{SimpleNicRxQueueDesc, SimpleNicTxQueueDesc};

use super::{ToCardQueue, ToCardQueueTyped, ToHostQueue, ToHostQueueTyped};

/// A transmit queue for the simple NIC device.
pub(crate) struct SimpleNicTxQueue {
    /// Inner queue
    inner: ToCardQueueTyped<SimpleNicTxQueueDesc>,
}

impl ToCardQueue for SimpleNicTxQueue {
    type Desc = SimpleNicTxQueueDesc;

    fn push(&mut self, desc: Self::Desc) -> Result<(), Self::Desc> {
        self.inner.push(desc)
    }
}

/// A receive queue for the simple NIC device.
pub(crate) struct SimpleNicRxQueue {
    /// Inner queue
    inner: ToHostQueueTyped<SimpleNicRxQueueDesc>,
}

impl ToHostQueue for SimpleNicRxQueue {
    type Desc = SimpleNicRxQueueDesc;

    fn pop(&mut self) -> Option<Self::Desc> {
        self.inner.pop()
    }
}

impl Deref for SimpleNicRxQueue {
    type Target = ToHostQueueTyped<SimpleNicRxQueueDesc>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for SimpleNicRxQueue {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
