package com.ffalcon.mercury.android.sdk.touch

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow

class TempleActionViewModel(application: Application) : AndroidViewModel(application) {
    private val _state = MutableSharedFlow<TempleAction>(extraBufferCapacity = 16)
    val userTempleAction = Channel<TempleAction>(Channel.UNLIMITED)
    val state: SharedFlow<TempleAction> = _state.asSharedFlow()
}
