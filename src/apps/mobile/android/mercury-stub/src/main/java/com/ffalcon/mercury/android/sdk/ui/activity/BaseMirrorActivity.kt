package com.ffalcon.mercury.android.sdk.ui.activity

import android.graphics.Color
import android.os.Bundle
import android.view.LayoutInflater
import android.view.ViewGroup
import android.widget.LinearLayout
import androidx.viewbinding.ViewBinding
import com.ffalcon.mercury.android.sdk.core.BindingPair
import java.lang.reflect.ParameterizedType

/**
 * Stub binocular host: inflates left/right ViewBinding panes side by side.
 * Replace with vendor Mercury AAR for real temple / launcher integration.
 */
abstract class BaseMirrorActivity<B : ViewBinding> : BaseEventActivity() {
    lateinit var mBindingPair: BindingPair<B>

    fun getMBindingPair(): BindingPair<B> = mBindingPair

    fun setMBindingPair(pair: BindingPair<B>) {
        mBindingPair = pair
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        initPair()
        setContentView(generateRootView())
    }

    @Suppress("UNCHECKED_CAST")
    private fun initPair() {
        val superType = javaClass.genericSuperclass
        require(superType is ParameterizedType) {
            "BaseMirrorActivity subclasses must declare a ViewBinding type argument"
        }
        val bindingClz = superType.actualTypeArguments[0] as Class<B>
        val inflate =
            bindingClz.getDeclaredMethod("inflate", LayoutInflater::class.java)
        inflate.isAccessible = true
        val left = inflate.invoke(null, layoutInflater) as B
        val right = inflate.invoke(null, layoutInflater) as B
        mBindingPair = BindingPair(left, right)
    }

    private fun generateRootView(): ViewGroup {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            setBackgroundColor(Color.BLACK)
            layoutParams =
                ViewGroup.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT,
                    ViewGroup.LayoutParams.MATCH_PARENT,
                )
        }
        val paneLp =
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 1f)
        root.addView(mBindingPair.left.root, paneLp)
        root.addView(mBindingPair.right.root, LinearLayout.LayoutParams(paneLp))
        return root
    }
}
