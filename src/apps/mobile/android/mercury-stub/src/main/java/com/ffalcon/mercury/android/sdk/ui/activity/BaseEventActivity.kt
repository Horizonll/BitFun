package com.ffalcon.mercury.android.sdk.ui.activity

import androidx.activity.viewModels
import androidx.appcompat.app.AppCompatActivity
import com.ffalcon.mercury.android.sdk.touch.TempleActionViewModel

open class BaseTouchActivity : AppCompatActivity()

open class BaseEventActivity : BaseTouchActivity() {
    private val templeActionViewModelInternal: TempleActionViewModel by viewModels()

    fun getTempleActionViewModel(): TempleActionViewModel = templeActionViewModelInternal

    val templeActionViewModel: TempleActionViewModel
        get() = templeActionViewModelInternal
}

open class BaseActivity : BaseEventActivity()
