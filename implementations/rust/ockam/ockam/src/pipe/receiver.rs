use crate::{
    pipe::{PipeBehavior, PipeModifier},
    protocols::pipe::{internal::InternalCmd, PipeMessage},
    Context,
};
use ockam_core::compat::boxed::Box;
use ockam_core::{Address, Any, Decodable, Result, Routed, Worker};

pub struct PipeReceiver {
    hooks: PipeBehavior,
    int_addr: Address,
}

#[crate::worker]
impl Worker for PipeReceiver {
    type Context = Context;
    type Message = Any;

    async fn initialize(&mut self, ctx: &mut Context) -> Result<()> {
        ctx.set_cluster(super::CLUSTER_NAME).await?;
        Ok(())
    }

    async fn handle_message(&mut self, ctx: &mut Context, msg: Routed<Any>) -> Result<()> {
        match msg.msg_addr() {
            addr if addr == self.int_addr => self.handle_internal(ctx, msg).await?,
            _ => self.handle_external(ctx, msg).await?,
        };

        Ok(())
    }
}

impl PipeReceiver {
    pub async fn create(
        ctx: &mut Context,
        addr: Address,
        int_addr: Address,
        hooks: PipeBehavior,
    ) -> Result<()> {
        ctx.start_worker(
            vec![addr, int_addr.clone()],
            PipeReceiver { hooks, int_addr },
        )
        .await
    }

    /// Handle external user messages
    async fn handle_external(&mut self, ctx: &mut Context, msg: Routed<Any>) -> Result<()> {
        // Convert Routed<Any> into PipeMessage + relevant metadata
        let return_route = msg.return_route();
        let pipe_msg = PipeMessage::decode(&msg.into_transport_message().payload)?;
        debug!(
            "Received pipe message with index '{}'",
            pipe_msg.index.u64()
        );

        // Before we send we give all hooks a chance to run
        match self
            .hooks
            .external_all(ctx.address(), return_route, ctx, &pipe_msg)
            .await
        {
            // Return early to prevent message sending if the
            // behaviour stack has determined to drop the message.
            Ok(PipeModifier::Drop) => return Ok(()),
            // On errors: don't crash the relay
            Err(e) => {
                warn!("Received message was invalid: {}!", e);
                return Ok(());
            }
            // Otherwise do nothing :)
            Ok(PipeModifier::None) => {}
        }

        // If we reach this point we can safely unpack and forward
        let msg = super::unpack_pipe_message(&pipe_msg)?;
        debug!("Forwarding message to {:?}", msg.transport().onward_route);
        ctx.forward(msg).await
    }

    async fn handle_internal(&mut self, ctx: &mut Context, msg: Routed<Any>) -> Result<()> {
        trace!("PipeReceiver receiving internal command");
        let return_route = msg.return_route();
        let trans = msg.into_transport_message();
        let internal_cmd = InternalCmd::from_transport(&trans)?;

        // Run the internal hooks for this receiver -- currently there
        // is only one pipe receiver hook: finish sender handshake
        self.hooks
            .internal_all(self.int_addr.clone(), return_route, ctx, &internal_cmd)
            .await
    }
}
