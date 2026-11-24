//! Test-only sender for isolated stock Ricochet servers with classic Metamod or Metamod-R.
//! Run sdk_message_probe after a real client is fully connected and recording.
use metamod_sdk_rs::prelude::*;

#[derive(Default)]
struct Delivery {
    registered: bool,
}

#[metamod_plugin(
    name = "SDK Message Delivery Probe",
    version = "1",
    author = "metamod-sdk-rs contributors",
    log_tag = "MSGDELIVERY"
)]
impl Plugin for Delivery {
    fn start_frame(&mut self, ctx: &mut Context<'_>) {
        if !self.registered {
            self.registered = ctx
                .server()
                .register_command(console_name!(c"sdk_message_probe"))
                .is_ok();
        }
    }

    fn server_command(&mut self, ctx: &mut Context<'_>, _: &Command) {
        for index in 1..=32 {
            let slot = PlayerSlot::new(index).unwrap();
            let Some(player) = ctx.player_at(slot) else {
                continue;
            };
            let mut text = UserMessagePayload::new();
            text.write_byte(2).unwrap(); // Ricochet HUD_PRINTCONSOLE.
            text.write_string(
                c"SDK_MESSAGE_INTEGRITY_254_255_16909060
",
            )
            .unwrap();
            for _ in 0..4 {
                text.write_string(c"").unwrap();
            }
            let mut fade = UserMessagePayload::new();
            // Stock ScreenFade is three little-endian shorts and four color bytes.
            // Zero alpha and duration make this protocol check visually inert.
            fade.write_short(0).unwrap();
            fade.write_short(0).unwrap();
            fade.write_short(0).unwrap();
            for value in [254, 255, 17, 0] {
                fade.write_byte(value).unwrap();
            }
            // SAFETY: This test runs only on the isolated stock Ricochet deployment
            // with classic Metamod or reviewed Metamod-R. Console dispatch is outside
            // message assembly. Only this sender and the recorder fixture are loaded;
            // neither installs message hooks. Direct engine
            // messaging is synchronous here. The operator waits for client signon.
            // Both payloads match the stock client parsers.
            let sender = unsafe { ctx.message_sender() };
            let text_result = sender.send_to(c"TextMsg", &text, player);
            let fade_result = sender.send_to(c"ScreenFade", &fade, player);
            ctx.metamod()
                .log_trunc(if text_result.is_ok() && fade_result.is_ok() {
                    c"MESSAGE_DELIVERY native sequences completed"
                } else {
                    c"MESSAGE_DELIVERY failed"
                });
        }
    }
}
