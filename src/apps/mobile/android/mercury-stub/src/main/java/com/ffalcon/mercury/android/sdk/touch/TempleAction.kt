package com.ffalcon.mercury.android.sdk.touch

/** Stub TempleAction sealed hierarchy matching MercurySDK public surface. */
sealed class TempleAction {
    var consumed: Boolean = false

    data object Idle : TempleAction()
    data object Click : TempleAction()
    data object LongClick : TempleAction()
    data object DoubleClick : TempleAction()
    data object TripleClick : TempleAction()
    data class SlideForward(val args: FlingArgs = FlingArgs()) : TempleAction()
    data class SlideBackward(val args: FlingArgs = FlingArgs()) : TempleAction()
    data class SlideUpwards(val args: FlingArgs = FlingArgs()) : TempleAction()
    data class SlideDownwards(val args: FlingArgs = FlingArgs()) : TempleAction()
    data class SlideContinuous(
        val delta: Float,
        val longClick: Boolean = false,
        val vertical: Boolean = false,
    ) : TempleAction()
    data class MoveUp(val isLongClick: Boolean = false) : TempleAction()
    data object ActionUp : TempleAction()
    data object ActionDown : TempleAction()
    data object DoubleFingerClick : TempleAction()
    data object DoubleFingerLongClick : TempleAction()
}

data class FlingArgs(
    val velocityX: Float = 0f,
    val velocityY: Float = 0f,
)
