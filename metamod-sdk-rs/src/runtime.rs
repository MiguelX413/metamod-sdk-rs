use metamod_sys::{
    Log, PluginInfo,
    raw::{MetamodGetUserMsgID, MetamodServerPrint},
};
use std::{
    error::Error,
    ffi::CStr,
    fmt,
    marker::PhantomData,
    num::{NonZeroI32, NonZeroU64, ParseIntError},
    str::FromStr,
};

/// An error parsing a player slot or user ID from text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParsePlayerNumberError<E> {
    /// The input could not be parsed as an `i32`.
    Integer(ParseIntError),
    /// The parsed number was zero or negative.
    Invalid(E),
}

impl<E: fmt::Display> fmt::Display for ParsePlayerNumberError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(error) => error.fmt(f),
            Self::Invalid(error) => error.fmt(f),
        }
    }
}

impl<E: Error + 'static> Error for ParsePlayerNumberError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Integer(error) => Some(error),
            Self::Invalid(error) => Some(error),
        }
    }
}

/// A one-based engine slot. GoldSrc can reuse it for a different player.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PlayerSlot(NonZeroI32);

impl PlayerSlot {
    /// Zero and negative indices return `None`. A valid index may be vacant;
    /// use `Context::player_at` to check occupancy.
    ///
    /// ```compile_fail,E0080
    /// use metamod_sdk_rs::PlayerSlot;
    /// const SLOT: PlayerSlot = PlayerSlot::new(0).unwrap();
    /// ```
    pub const fn new(index: i32) -> Option<Self> {
        match NonZeroI32::new(index) {
            Some(index) if index.get() > 0 => Some(Self(index)),
            _ => None,
        }
    }

    /// Return the positive, one-based GoldSrc slot index.
    pub const fn get(self) -> i32 {
        self.0.get()
    }
}

impl fmt::Display for PlayerSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.get(), f)
    }
}

/// The supplied index was zero or negative.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidPlayerSlot(i32);

impl fmt::Display for InvalidPlayerSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "player slot index must be positive, got {}", self.0)
    }
}

impl std::error::Error for InvalidPlayerSlot {}

impl TryFrom<i32> for PlayerSlot {
    type Error = InvalidPlayerSlot;

    fn try_from(index: i32) -> Result<Self, Self::Error> {
        Self::new(index).ok_or(InvalidPlayerSlot(index))
    }
}

impl FromStr for PlayerSlot {
    type Err = ParsePlayerNumberError<InvalidPlayerSlot>;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let number = text
            .parse::<i32>()
            .map_err(ParsePlayerNumberError::Integer)?;
        Self::try_from(number).map_err(ParsePlayerNumberError::Invalid)
    }
}

impl From<PlayerSlot> for i32 {
    fn from(slot: PlayerSlot) -> Self {
        slot.get()
    }
}

/// A positive, server-assigned GoldSrc user ID. This is separate from an
/// edict slot or a map-scoped player session handle.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PlayerUserId(NonZeroI32);

impl PlayerUserId {
    /// Zero and negative values represent unavailable or invalid IDs and
    /// return `None`.
    ///
    /// ```compile_fail,E0080
    /// use metamod_sdk_rs::PlayerUserId;
    /// const USER_ID: PlayerUserId = PlayerUserId::new(-1).unwrap();
    /// ```
    pub const fn new(value: i32) -> Option<Self> {
        match NonZeroI32::new(value) {
            Some(value) if value.get() > 0 => Some(Self(value)),
            _ => None,
        }
    }

    /// Positive native user ID.
    pub const fn get(self) -> i32 {
        self.0.get()
    }
}

impl fmt::Display for PlayerUserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.get(), f)
    }
}

/// The supplied GoldSrc user ID was zero or negative.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidPlayerUserId(i32);

impl fmt::Display for InvalidPlayerUserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "player user ID must be positive, got {}", self.0)
    }
}

impl std::error::Error for InvalidPlayerUserId {}

impl TryFrom<i32> for PlayerUserId {
    type Error = InvalidPlayerUserId;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        Self::new(value).ok_or(InvalidPlayerUserId(value))
    }
}

impl FromStr for PlayerUserId {
    type Err = ParsePlayerNumberError<InvalidPlayerUserId>;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let number = text
            .parse::<i32>()
            .map_err(ParsePlayerNumberError::Integer)?;
        Self::try_from(number).map_err(ParsePlayerNumberError::Invalid)
    }
}

impl From<PlayerUserId> for i32 {
    fn from(id: PlayerUserId) -> Self {
        id.get()
    }
}

/// A player session observed by this plugin, scoped to a map.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PlayerId {
    slot: PlayerSlot,
    serial: NonZeroU64,
}

impl PlayerId {
    pub(crate) fn new(slot: PlayerSlot, serial: NonZeroU64) -> Self {
        Self { slot, serial }
    }

    /// Return the one-based slot. Slots are reused; compare the complete
    /// [`PlayerId`] when distinguishing sessions.
    pub fn slot(self) -> PlayerSlot {
        self.slot
    }

    /// The one-based GoldSrc slot index. Use the full ID for identity comparisons.
    pub fn get(self) -> i32 {
        self.slot.get()
    }
}

/// Access to plugin services during a callback on the GoldSrc server thread.
///
/// ```compile_fail
/// use metamod_sdk_rs::Context;
/// fn retain<'a>(context: &'a Context<'_>) -> &'a Context<'static> {
///     context
/// }
/// ```
///
/// ```compile_fail
/// use metamod_sdk_rs::Context;
/// fn send_to_other_thread(context: &Context<'_>) {
///     std::thread::scope(|scope| {
///         let server = context.server();
///         scope.spawn(move || server.print(c"invalid thread"));
///     });
/// }
/// ```
pub struct Context<'callback> {
    pub(crate) id: *mut PluginInfo,
    pub(crate) user_message: MetamodGetUserMsgID,
    log: Log,
    console: Log,
    print: MetamodServerPrint,
    pub(crate) command: crate::engine::CommandCallback,
    scope: PhantomData<&'callback ()>,
}

/// Scoped access to server operations.
pub struct Server<'context, 'callback> {
    pub(crate) context: &'context Context<'callback>,
}

// Classic Win32 Metamod uses _vsnprintf, which can leave its 1,024-byte
// buffer unterminated. Leave room for the console utility's newline.
const MAX_UTILITY_MESSAGE: usize = 1021;

/// A C string validated for Metamod's logging and console utilities.
#[repr(transparent)]
#[derive(Debug)]
pub struct MetamodMessage(CStr);

/// A message exceeded the Metamod utility payload limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetamodMessageTooLong;

impl fmt::Display for MetamodMessageTooLong {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Metamod utility messages may contain at most {MAX_UTILITY_MESSAGE} bytes"
        )
    }
}

impl Error for MetamodMessageTooLong {}

impl MetamodMessage {
    /// Maximum payload length in bytes, excluding the NUL terminator.
    pub const MAX_LEN: usize = MAX_UTILITY_MESSAGE;

    /// The validated C string.
    pub fn as_c_str(&self) -> &CStr {
        &self.0
    }
}

impl<'a> TryFrom<&'a CStr> for &'a MetamodMessage {
    type Error = MetamodMessageTooLong;

    fn try_from(message: &'a CStr) -> Result<Self, Self::Error> {
        if message.to_bytes().len() > MetamodMessage::MAX_LEN {
            return Err(MetamodMessageTooLong);
        }
        // SAFETY: The transparent wrapper preserves CStr's layout and pointer
        // metadata. Its length invariant holds, and the borrow is unchanged.
        Ok(unsafe { &*(message as *const CStr as *const MetamodMessage) })
    }
}

/// Scoped access to Metamod's logging and console utilities.
pub struct MetaMod<'context, 'callback> {
    pub(crate) context: &'context Context<'callback>,
}

fn utility_message<'a>(
    message: &'a CStr,
    buffer: &'a mut [u8; MAX_UTILITY_MESSAGE + 1],
) -> &'a MetamodMessage {
    if let Ok(message) = <&MetamodMessage>::try_from(message) {
        return message;
    }
    buffer[..MAX_UTILITY_MESSAGE].copy_from_slice(&message.to_bytes()[..MAX_UTILITY_MESSAGE]);
    buffer[MAX_UTILITY_MESSAGE] = 0;
    CStr::from_bytes_with_nul(buffer)
        .expect("a C string payload contains no NUL")
        .try_into()
        .expect("the truncated message fits the utility limit")
}

impl<'callback> Context<'callback> {
    pub(crate) fn new(
        _: &'callback (),
        id: *mut PluginInfo,
        log: Log,
        console: Log,
        user_message: MetamodGetUserMsgID,
        print: MetamodServerPrint,
        command: crate::engine::CommandCallback,
    ) -> Self {
        Self {
            id,
            log,
            console,
            user_message,
            print,
            command,
            scope: PhantomData,
        }
    }

    /// Access callback-scoped engine services such as commands, CVars,
    /// and traces. Individual operations can be unavailable. Entity discovery
    /// is available directly on [`Context`].
    pub fn server(&self) -> Server<'_, 'callback> {
        Server { context: self }
    }

    /// Access Metamod's logging and console utilities.
    ///
    /// ```
    /// use metamod_sdk_rs::prelude::*;
    ///
    /// fn announce(ctx: &Context<'_>, text: &std::ffi::CStr) {
    ///     let metamod = ctx.metamod();
    ///     metamod.print_console_trunc(text);
    ///     metamod.log_trunc(text);
    /// }
    /// ```
    ///
    /// To reject oversized messages, convert to [`MetamodMessage`] and use
    /// the methods that accept it.
    pub fn metamod(&self) -> MetaMod<'_, 'callback> {
        MetaMod { context: self }
    }

    /// Look up the currently observed session in a reusable slot.
    ///
    /// Keep the full session ID when scheduling work for a later callback. Looking
    /// up the slot again would identify its new occupant after a reconnect.
    ///
    /// ```
    /// use metamod_sdk_rs::{Context, PlayerId, PlayerSlot};
    ///
    /// #[derive(Default)]
    /// struct Selection {
    ///     player: Option<PlayerId>,
    /// }
    /// impl Selection {
    ///     fn select(&mut self, ctx: &Context<'_>, slot: PlayerSlot) {
    ///         self.player = ctx.player_at(slot);
    ///     }
    ///     fn current(&self, ctx: &Context<'_>) -> Option<PlayerId> {
    ///         self.player.filter(|id| ctx.is_player_current(*id))
    ///     }
    /// }
    /// ```
    pub fn player_at(&self, slot: PlayerSlot) -> Option<PlayerId> {
        crate::__ffi::player_at(slot)
    }

    /// Whether this exact session is still tracked on the current map.
    pub fn is_player_current(&self, player: PlayerId) -> bool {
        self.player_at(player.slot()) == Some(player)
    }

    /// Write a message through Metamod's server log utility when available.
    /// Messages exceeding 1,021 bytes are truncated to fit classic Metamod.
    pub fn log(&self, message: &CStr) {
        self.metamod().log_trunc(message);
    }

    /// Print directly to the server console through Metamod.
    /// Messages exceeding 1,021 bytes are truncated to fit classic Metamod.
    pub fn print_console(&self, message: &CStr) {
        self.metamod().print_console_trunc(message);
    }
}

impl MetaMod<'_, '_> {
    /// Write a validated message through Metamod's server log utility.
    pub fn log(&self, message: &MetamodMessage) {
        self.write(self.context.log, message);
    }

    /// Print a validated message through Metamod's console utility.
    ///
    /// Reject an oversized message instead of silently truncating it:
    ///
    /// ```
    /// use metamod_sdk_rs::{Context, MetamodMessage, MetamodMessageTooLong};
    /// use std::ffi::CStr;
    ///
    /// fn announce(ctx: &Context<'_>, text: &CStr) -> Result<(), MetamodMessageTooLong> {
    ///     let validated: &MetamodMessage = text.try_into()?;
    ///     ctx.metamod().print_console(validated);
    ///     Ok(())
    /// }
    /// ```
    pub fn print_console(&self, message: &MetamodMessage) {
        self.write(self.context.console, message);
    }

    /// Log a C string, truncating by bytes if it exceeds the utility limit.
    pub fn log_trunc(&self, message: &CStr) {
        self.write_trunc(self.context.log, message);
    }

    /// Print a C string, truncating by bytes if it exceeds the utility limit.
    pub fn print_console_trunc(&self, message: &CStr) {
        self.write_trunc(self.context.console, message);
    }

    fn write_trunc(&self, callback: Log, message: &CStr) {
        let mut buffer = [0; MAX_UTILITY_MESSAGE + 1];
        self.write(callback, utility_message(message, &mut buffer));
    }

    fn write(&self, callback: Log, message: &MetamodMessage) {
        if crate::__ffi::engine_available()
            && let Some(log) = callback
        {
            // SAFETY: The loader supplied the callback and plugin ID.
            // The C varargs match the format and message lives through the call.
            unsafe { log(self.context.id, c"%s".as_ptr(), message.as_c_str().as_ptr()) };
        }
    }
}

impl Server<'_, '_> {
    /// Pass native bytes directly to the engine console. Include a newline
    /// for line output. This does nothing when the engine callback is unavailable.
    pub fn print(&self, message: &CStr) {
        if crate::__ffi::engine_available()
            && let Some(print) = self.context.print
        {
            // SAFETY: The engine callback is valid on the server thread,
            // and the C string lives through the synchronous call.
            unsafe { print(message.as_ptr()) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metamod_message_borrows_bytes_and_checks_the_inclusive_limit() {
        for len in [0, MetamodMessage::MAX_LEN] {
            let input = std::ffi::CString::new(vec![0xff; len]).unwrap();
            let message = <&MetamodMessage>::try_from(input.as_c_str()).unwrap();
            assert_eq!(message.as_c_str(), input.as_c_str());
            assert_eq!(message.as_c_str().as_ptr(), input.as_ptr());
        }
        let input = std::ffi::CString::new(vec![b'x'; MetamodMessage::MAX_LEN + 1]).unwrap();
        assert_eq!(
            <&MetamodMessage>::try_from(input.as_c_str()).unwrap_err(),
            MetamodMessageTooLong
        );
    }

    #[test]
    fn utility_messages_leave_room_for_a_newline_in_classic_metamod() {
        let long = std::ffi::CString::new(vec![0xff; MAX_UTILITY_MESSAGE + 1]).unwrap();
        let mut storage = [b'x'; MAX_UTILITY_MESSAGE + 1];
        let truncated = utility_message(&long, &mut storage).as_c_str();
        assert_eq!(
            truncated.to_bytes(),
            &long.to_bytes()[..MAX_UTILITY_MESSAGE]
        );
        assert_eq!(truncated.to_bytes_with_nul().last(), Some(&0));
        let short = c"short";
        assert_eq!(
            utility_message(short, &mut storage).as_c_str().as_ptr(),
            short.as_ptr()
        );
    }
    use std::num::IntErrorKind;

    fn check_player_number<T, E>()
    where
        T: FromStr<Err = ParsePlayerNumberError<E>> + Into<i32>,
        E: fmt::Debug,
    {
        for (text, value) in [("7", 7), ("+007", 7), ("2147483647", i32::MAX)] {
            assert_eq!(text.parse::<T>().unwrap().into(), value);
        }
        for (text, kind) in [
            ("", IntErrorKind::Empty),
            ("abc", IntErrorKind::InvalidDigit),
            (" 7", IntErrorKind::InvalidDigit),
            ("1_000", IntErrorKind::InvalidDigit),
            ("0x10", IntErrorKind::InvalidDigit),
            ("2147483648", IntErrorKind::PosOverflow),
            ("-2147483649", IntErrorKind::NegOverflow),
        ] {
            assert!(
                matches!(
                    text.parse::<T>(),
                    Err(ParsePlayerNumberError::Integer(error)) if *error.kind() == kind
                ),
                "{text:?}"
            );
        }
        for text in ["0", "-0", "-1", "-2147483648"] {
            assert!(
                matches!(text.parse::<T>(), Err(ParsePlayerNumberError::Invalid(_))),
                "{text:?}"
            );
        }
    }

    #[test]
    fn player_slots_parse_standard_integers() {
        check_player_number::<PlayerSlot, InvalidPlayerSlot>();
    }

    #[test]
    fn player_user_ids_parse_standard_integers() {
        check_player_number::<PlayerUserId, InvalidPlayerUserId>();
    }
}
