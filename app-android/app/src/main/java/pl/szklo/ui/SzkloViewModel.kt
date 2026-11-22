package pl.szklo.ui

import android.app.Application
import android.content.Intent
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import pl.szklo.audio.AudioHub
import pl.szklo.audio.GlobalEffectsService
import pl.szklo.audio.GlobalEffectsState
import pl.szklo.audio.LocalPlayerService
import pl.szklo.audio.MeterReading
import pl.szklo.audio.PlayerState
import pl.szklo.core.NativeEngine
import pl.szklo.model.EngineParams
import pl.szklo.model.ModuleGroup
import pl.szklo.model.ModuleState
import pl.szklo.model.Preset
import pl.szklo.model.Presets
import kotlin.math.roundToInt

/**
 * Stan ekranu: 23 moduły, presety, moc ogólna, mierniki (10 Hz), wyjście audio, odtwarzacz, tryb
 * „inne aplikacje”. Parametry żyją w [AudioHub] (współdzielone z usługami); tu są widoki i akcje.
 */
class SzkloViewModel(app: Application) : AndroidViewModel(app) {

    private val hub = AudioHub

    val modules: StateFlow<List<ModuleState>> = hub.params.map { it.modules }.distinctUntilChanged()
        .stateIn(viewModelScope, SharingStarted.Eagerly, hub.params.value.modules)

    val master: StateFlow<Float> = hub.params.map { it.master }.distinctUntilChanged()
        .stateIn(viewModelScope, SharingStarted.Eagerly, hub.params.value.master)

    val enabled: StateFlow<Boolean> = hub.params.map { it.enabled }.distinctUntilChanged()
        .stateIn(viewModelScope, SharingStarted.Eagerly, hub.params.value.enabled)

    val presetKey: StateFlow<String> = hub.presetKey

    val userPresets: StateFlow<List<Preset>> = hub.userPresets

    val player: StateFlow<PlayerState> = hub.player

    val global: StateFlow<GlobalEffectsState> = hub.global

    /** Wybrana grupa w panelu dolnym (makieta: „Dynamika”). */
    private val _group = MutableStateFlow(ModuleGroup.DYNAMIKA)
    val group: StateFlow<ModuleGroup> = _group.asStateFlow()

    /** Odczyt mierników 10 Hz – tylko gdy UI obserwuje (WhileSubscribed). */
    val meters: StateFlow<MeterReading?> = flow {
        while (true) {
            emit(hub.readMeters())
            delay(METER_PERIOD_MS)
        }
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(1_000), null)

    private val _outputDevice = MutableStateFlow(OUTPUT_SPEAKER)
    val outputDevice: StateFlow<String> = _outputDevice.asStateFlow()

    /** Linia stanu pod pokrętłem: „Słuchawki BT · −11,0 LUFS · szczyt −1,0 dBTP”. */
    val statusLine: StateFlow<String> = combine(meters, outputDevice, global, hub.params) { m, dev, g, p ->
        when {
            m != null -> "$dev · ${Fmt.db(m.lufs3s)} LUFS · szczyt ${Fmt.db(m.truePeakDb)} dBTP"
            g.running && g.attached.isNotEmpty() -> "$dev · inne aplikacje · sufit ${Fmt.db(p.ceilingDb)} dBTP"
            else -> "$dev · cisza · sufit ${Fmt.db(p.ceilingDb)} dBTP"
        }
    }.distinctUntilChanged().stateIn(viewModelScope, SharingStarted.WhileSubscribed(1_000), "$OUTPUT_SPEAKER · cisza · sufit −1,0 dBTP")

    val coreAvailable: Boolean get() = NativeEngine.isAvailable

    private var presetJob: Job? = null

    // ------------------------------------------------------------------ wyjście audio

    private val audioManager = app.getSystemService(AudioManager::class.java)
    private val deviceCallback = object : AudioDeviceCallback() {
        override fun onAudioDevicesAdded(addedDevices: Array<out AudioDeviceInfo>?) = refreshOutput()
        override fun onAudioDevicesRemoved(removedDevices: Array<out AudioDeviceInfo>?) = refreshOutput()
    }

    init {
        audioManager.registerAudioDeviceCallback(deviceCallback, Handler(Looper.getMainLooper()))
        refreshOutput()
    }

    override fun onCleared() {
        audioManager.unregisterAudioDeviceCallback(deviceCallback)
        super.onCleared()
    }

    private fun refreshOutput() {
        val outs = audioManager.getDevices(AudioManager.GET_DEVICES_OUTPUTS)
        fun has(vararg types: Int) = outs.any { it.type in types }
        val bleHeadset = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            has(AudioDeviceInfo.TYPE_BLE_HEADSET, AudioDeviceInfo.TYPE_BLE_SPEAKER)
        } else {
            false
        }
        _outputDevice.value = when {
            bleHeadset || has(AudioDeviceInfo.TYPE_BLUETOOTH_A2DP) -> "Słuchawki BT"
            has(AudioDeviceInfo.TYPE_WIRED_HEADPHONES, AudioDeviceInfo.TYPE_WIRED_HEADSET) -> "Słuchawki"
            has(AudioDeviceInfo.TYPE_USB_HEADSET, AudioDeviceInfo.TYPE_USB_DEVICE) -> "USB audio"
            else -> OUTPUT_SPEAKER
        }
    }

    // ------------------------------------------------------------------ moduły i moc

    fun selectGroup(g: ModuleGroup) {
        _group.value = g
    }

    fun setModuleOn(index: Int, on: Boolean) {
        presetJob?.cancel()
        hub.updateParams { p -> p.withModule(index) { it.copy(on = on) } }
    }

    fun setModuleAmount(index: Int, amount: Float) {
        presetJob?.cancel()
        val a = (amount * 100f).roundToInt().coerceIn(0, 100) / 100f // krok 1%
        hub.updateParams { p -> p.withModule(index) { it.copy(amount = a) } }
    }

    fun setMaster(value: Float) {
        presetJob?.cancel()
        val v = value.coerceIn(0f, 1f)
        hub.updateParams { it.copy(master = v) }
    }

    /** Podwójne stuknięcie w pokrętło: moc ogólna z bieżącego presetu. */
    fun resetMasterToPreset() {
        val preset = hub.presetByKey(presetKey.value) ?: Presets.default
        setMaster(preset.master)
    }

    fun setEnabled(on: Boolean) {
        hub.updateParams { it.copy(enabled = on) }
    }

    /** Przytrzymanie „porównaj z oryginałem”. */
    fun setCompare(hold: Boolean) {
        hub.updateParams { it.copy(compare = hold) }
    }

    /** Wczytanie presetu z płynnym przejściem 150 ms (jak na PC). On/off przełącza się od razu. */
    fun applyPreset(key: String) {
        val target = hub.presetByKey(key) ?: return
        presetJob?.cancel()
        hub.setPresetKey(key)
        val from = hub.params.value
        presetJob = viewModelScope.launch {
            val start = SystemClock.uptimeMillis()
            while (true) {
                val t = ((SystemClock.uptimeMillis() - start) / PRESET_FADE_MS).coerceIn(0f, 1f)
                val e = t * t * (3f - 2f * t) // smoothstep
                hub.updateParams { interpolate(from, target, e, it) }
                if (t >= 1f) break
                delay(16)
            }
        }
    }

    private fun interpolate(from: EngineParams, to: Preset, e: Float, current: EngineParams): EngineParams {
        val mods = List(from.modules.size) { i ->
            val a = from.modules[i]
            val b = to.modules[i]
            ModuleState(on = b.on, amount = a.amount + (b.amount - a.amount) * e)
        }
        return current.copy(modules = mods, master = from.master + (to.master - from.master) * e)
    }

    // ------------------------------------------------------------------ odtwarzacz lokalny

    fun playLocal(uri: Uri) {
        val ctx = getApplication<Application>()
        // Dostęp do pliku po restarcie usługi/procesu (ACTION_OPEN_DOCUMENT).
        runCatching { ctx.contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION) }
        LocalPlayerService.play(ctx, uri)
    }

    fun pauseLocal() = LocalPlayerService.command(getApplication<Application>(), LocalPlayerService.ACTION_PAUSE)
    fun resumeLocal() = LocalPlayerService.command(getApplication<Application>(), LocalPlayerService.ACTION_RESUME)
    fun stopLocal() = LocalPlayerService.command(getApplication<Application>(), LocalPlayerService.ACTION_STOP)

    // ------------------------------------------------------------------ inne aplikacje

    fun setGlobalEffects(on: Boolean) {
        hub.setGlobalRequested(on)
        val ctx = getApplication<Application>()
        if (on) GlobalEffectsService.start(ctx) else GlobalEffectsService.stop(ctx)
    }

    private companion object {
        const val METER_PERIOD_MS = 100L
        const val PRESET_FADE_MS = 150f
        const val OUTPUT_SPEAKER = "Głośnik"
    }
}
