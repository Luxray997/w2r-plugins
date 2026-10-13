//! Build 1.0.2.2818 addresses and the checks that must pass before the probe calls into the SDK.
//! The numbers come from Ghidra (docs/battlenet-stats-research.md in the w2rpm repository): the
//! exe keeps a static singleton of ClientSdk service wrappers, each `{vftable, sdk object*}`;
//! `LocalWebsocketServerService` is at +0xf8, and the SDK object's vtable is `LocalWebsocketServerV1`.

/// `Warcraft II.exe` preferred image base and `ClientSdk.dll` preferred image base.
pub const EXE_IMAGE_BASE: u32 = 0x0040_0000;
pub const SDK_IMAGE_BASE: u32 = 0x1000_0000;

/// The singleton of service wrappers (`FUN_00594860` returns its address).
pub const CONTAINER_VA: u32 = 0x0096_4d78;
/// `LocalWebsocketServerService` inside it, and the field holding the SDK object pointer.
pub const WRAPPER_OFFSET: u32 = 0xf8;
pub const SDK_OBJECT_FIELD: u32 = 0x04;
/// `LocalWebsocketServerService::vftable` (exe).
pub const WRAPPER_VTABLE_VA: u32 = 0x0086_955c;
/// `LocalWebsocketServerV1::vftable` (SDK), and vtable slot 17 (no arguments): registers the `web-api/(.*)` route.
pub const OBJECT_VTABLE_VA: u32 = 0x10ab_14b8;
pub const START_SLOT: u32 = 17;
pub const START_FN_VA: u32 = 0x102e_1ca0;
/// Every slot of that vtable (slot 0 is the destructor), as read from the live game and the dump.
pub const SLOT_VAS: [u32; 18] = [
    0x102d_bd50, 0x1030_9cd0, 0x102d_bf00, 0x102e_9100, 0x1030_6770, 0x102d_fb10, 0x1030_bce0, 0x102d_dd70,
    0x102e_aad0, 0x102e_70b0, 0x102e_54e0, 0x102e_3710, 0x1030_82f0, 0x1030_da80, 0x1030_4a60, 0x1030_2ea0,
    0x1030_1490, 0x102e_1ca0,
];

/// Runtime address of a static in a module that was loaded at `base`.
pub fn runtime(va: u32, image_base: u32, base: u32) -> u32 {
    base.wrapping_add(va.wrapping_sub(image_base))
}

/// What was read out of the live game.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Observed {
    pub exe_base: u32,
    pub sdk_base: u32,
    pub wrapper_vtable: u32,
    pub sdk_object: u32,
    pub object_vtable: u32,
    pub start_fn: u32,
}

/// `Ok` when every pointer is where this build's analysis says it is, so calling slot 17 is the
/// call that was analysed. The first mismatch is the error.
pub fn verify(o: &Observed) -> Result<(), &'static str> {
    if o.exe_base == 0 {
        return Err("game module not found");
    }
    if o.sdk_base == 0 {
        return Err("ClientSdk.dll not loaded");
    }
    if o.wrapper_vtable != runtime(WRAPPER_VTABLE_VA, EXE_IMAGE_BASE, o.exe_base) {
        return Err("service wrapper vtable differs: not build 2818, or not initialised yet");
    }
    if o.sdk_object == 0 {
        return Err("SDK object pointer is null");
    }
    if o.object_vtable != runtime(OBJECT_VTABLE_VA, SDK_IMAGE_BASE, o.sdk_base) {
        return Err("SDK object vtable differs from LocalWebsocketServerV1");
    }
    if o.start_fn != runtime(START_FN_VA, SDK_IMAGE_BASE, o.sdk_base) {
        return Err("vtable slot 17 is not the analysed function");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good() -> Observed {
        let (exe_base, sdk_base) = (0x006d_0000, 0x53f7_0000);
        Observed {
            exe_base,
            sdk_base,
            wrapper_vtable: exe_base + (WRAPPER_VTABLE_VA - EXE_IMAGE_BASE),
            sdk_object: 0x056a_8d28,
            object_vtable: sdk_base + (OBJECT_VTABLE_VA - SDK_IMAGE_BASE),
            start_fn: sdk_base + (START_FN_VA - SDK_IMAGE_BASE),
        }
    }

    #[test]
    fn the_addresses_seen_in_the_live_game_pass() {
        // Values from a real run: exe at 0x6d0000, SDK at 0x53f70000, container at 0xc34d78.
        let o = good();
        assert_eq!(runtime(CONTAINER_VA, EXE_IMAGE_BASE, o.exe_base), 0x00c3_4d78);
        assert_eq!(runtime(OBJECT_VTABLE_VA, SDK_IMAGE_BASE, o.sdk_base), 0x54a2_14b8);
        assert_eq!(verify(&o), Ok(()));
    }

    #[test]
    fn every_mismatch_is_refused() {
        let cases: [(fn(&mut Observed), &str); 6] = [
            (|o| o.exe_base = 0, "game module not found"),
            (|o| o.sdk_base = 0, "ClientSdk.dll not loaded"),
            (|o| o.wrapper_vtable += 4, "service wrapper vtable differs: not build 2818, or not initialised yet"),
            (|o| o.sdk_object = 0, "SDK object pointer is null"),
            (|o| o.object_vtable += 4, "SDK object vtable differs from LocalWebsocketServerV1"),
            (|o| o.start_fn += 1, "vtable slot 17 is not the analysed function"),
        ];
        for (mutate, why) in cases {
            let mut o = good();
            mutate(&mut o);
            assert_eq!(verify(&o), Err(why));
        }
    }

    #[test]
    fn the_slot_table_agrees_with_the_start_slot() {
        assert_eq!(SLOT_VAS[START_SLOT as usize], START_FN_VA);
        assert_eq!(SLOT_VAS.len(), 18);
    }

    #[test]
    fn the_start_slot_offset_matches_the_vtable_layout() {
        assert_eq!(START_SLOT * 4, 0x44);
        assert_eq!(WRAPPER_OFFSET + SDK_OBJECT_FIELD, 0xfc);
    }
}
