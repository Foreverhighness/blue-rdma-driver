use tracing::error;

use crate::{
    ack_responder::AckResponse,
    completion_v3::{
        Completion, Event, MessageMeta, RecvEvent, RecvEventOp, SendEvent, SendEventOp,
    },
    device_protocol::{HeaderReadMeta, HeaderType, HeaderWriteMeta, PacketPos, WorkReqOpCode},
    message_worker::Task,
    queue_pair::num_psn,
    send::{SendWrBase, SendWrRdma},
    tracker::Msn,
};

use super::{CompletionTask, MetaWorker, RdmaWriteTask};

impl<T> MetaWorker<T> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_header_write(&mut self, meta: HeaderWriteMeta) {
        let HeaderWriteMeta {
            pos,
            msn,
            psn,
            solicited,
            ack_req,
            is_retry,
            dqpn,
            total_len,
            raddr,
            rkey,
            imm,
            header_type,
        } = meta;
        let tracker = self
            .recv_table
            .get_mut(dqpn)
            .unwrap_or_else(|| unreachable!("qp number: d{dqpn} does not exist"));

        if matches!(pos, PacketPos::Last | PacketPos::Only) {
            match header_type {
                HeaderType::Write => {}
                HeaderType::WriteWithImm => {
                    let event = Event::Recv(RecvEvent::new(
                        RecvEventOp::WriteWithImm { imm },
                        MessageMeta::new(msn, psn),
                    ));
                    let _ignore = self
                        .completion_tx
                        .send(CompletionTask::Register { qpn: dqpn, event });
                }
                HeaderType::Send => {
                    let event = Event::Recv(RecvEvent::new(
                        RecvEventOp::Recv,
                        MessageMeta::new(msn, psn),
                    ));
                    let _ignore = self
                        .completion_tx
                        .send(CompletionTask::Register { qpn: dqpn, event });
                }
                HeaderType::ReadResp => {
                    let event = Event::Recv(RecvEvent::new(
                        RecvEventOp::ReadResp,
                        MessageMeta::new(msn, psn),
                    ));
                    let _ignore = self
                        .completion_tx
                        .send(CompletionTask::Register { qpn: dqpn, event });
                }
            }
        }
        if let Some(base_psn) = tracker.ack_one(psn) {
            let _ignore = self.completion_tx.send(CompletionTask::Ack {
                qpn: dqpn,
                base_psn,
                is_send: false,
            });
        }
        /// Timeout of an `AckReq` message, notify retransmission
        if matches!(pos, PacketPos::Last | PacketPos::Only) && is_retry && ack_req {
            let _ignore = self.ack_tx.send(AckResponse::Nak {
                qpn: dqpn,
                base_psn: tracker.base_psn(),
                ack_req_packet_psn: psn,
            });
        }
    }

    pub(super) fn handle_header_read(&self, meta: HeaderReadMeta) {
        let flags = if meta.ack_req {
            ibverbs_sys::ibv_send_flags::IBV_SEND_SOLICITED.0
        } else {
            0
        };
        let base = SendWrBase::new(0, flags, meta.raddr, meta.total_len, meta.rkey, 0);
        let send_wr = SendWrRdma::new_from_base(base, meta.laddr, meta.lkey);
        let (task, _) = RdmaWriteTask::new(meta.dqpn, send_wr, WorkReqOpCode::RdmaReadResp);
        let _ignore = self.rdma_write_tx.send(task);
    }
}
