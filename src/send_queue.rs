use std::{cmp::Ordering, collections::VecDeque, iter};

use crate::{
    constants::{MAX_PSN_WINDOW, MAX_QP_CNT},
    device_protocol::{QpParams, WorkReqOpCode},
    queue_pair::qpn_index,
    send::SendWrRdma,
};

#[derive(Default)]
pub(crate) struct IbvSendQueue {
    inner: VecDeque<SendQueueElem>,
}

impl IbvSendQueue {
    pub(crate) fn push(&mut self, elem: SendQueueElem) {
        self.inner.push_back(elem);
    }

    pub(crate) fn pop_until(&mut self, psn: u32) {
        let mut a = self.inner.partition_point(|x| x.psn < Psn(psn));
        let _drop = self.inner.drain(..a);
    }

    /// Find range [`psn_low`, `psn_high`)
    pub(crate) fn range(&self, psn_low: u32, psn_high: u32) -> Vec<SendQueueElem> {
        let mut a = self.inner.partition_point(|x| x.psn <= Psn(psn_low));
        let mut b = self.inner.partition_point(|x| x.psn < Psn(psn_high));
        a = a.saturating_sub(1);
        self.inner.range(a..b).copied().collect()
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SendQueueElem {
    psn: Psn,
    wr: SendWrRdma,
    qp_param: QpParams,
}

impl SendQueueElem {
    pub(crate) fn new(wr: SendWrRdma, psn: u32, qp_param: QpParams) -> Self {
        Self {
            wr,
            psn: Psn(psn),
            qp_param,
        }
    }

    pub(crate) fn psn(&self) -> u32 {
        self.psn.0
    }

    pub(crate) fn wr(&self) -> SendWrRdma {
        self.wr
    }

    pub(crate) fn qp_param(&self) -> QpParams {
        self.qp_param
    }

    pub(crate) fn opcode(&self) -> WorkReqOpCode {
        self.wr.opcode()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Psn(pub(crate) u32);

impl PartialOrd for Psn {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let x = self.0.wrapping_sub(other.0);
        Some(match x {
            0 => Ordering::Equal,
            x if x as usize > MAX_PSN_WINDOW => Ordering::Less,
            _ => Ordering::Greater,
        })
    }
}
