package pl.szklo.audio

import android.content.Context
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import pl.szklo.core.NativeEngine
import pl.szklo.model.EngineParams
import pl.szklo.model.ModuleId
import pl.szklo.model.ParamsCodec
import pl.szklo.model.Preset
import pl.szklo.model.Presets

/** Stan lokalnego odtwarzacza (LocalPlayerService). */
enum class PlayerStatus { IDLE, PREPARING, PLAYING, PAUSED, ERROR }

data class PlayerState(
    val status: PlayerStatus = PlayerStatus.IDLE,
    val title: String? = null,
    val positionMs: Long = 0,
    val durationMs: Long = -1,
    val sampleRate: Int = 0,
    val channels: Int = 0,
    /** false = gra bez obróbki, bo brak libszklo_jni.so. */
    val processed: Boolean = true,
    val error: String? = null,
)

/** Odczyt mierników rdzenia (wartości jak w `nativeMeters`). */
data class MeterReading(
    val inDb: Float,
    val outDb: Float,
    val grDb: Float,
    val lufs3s: Float,
    val truePeakDb: Float,
)

/** Stan odbiornika Spotify Connect „Szkło” (SpotifyConnectService). */
data class SpotifyState(
    /** Usługa działa (odbiornik uruchomiony). */
    val running: Boolean = false,
    /** Stan z rdzenia: idle | connecting | login | ready | playing | error. */
    val status: String = "idle",
    /** Adres logowania do otwarcia w przeglądarce (gdy status = login). */
    val loginUrl: String? = null,
    val error: String? = null,
)

/** Stan mostu efektów dla innych aplikacji (GlobalEffectsService). */
data class GlobalEffectsState(
    /** Wybór użytkownika (zapisywany). */
    val requested: Boolean = true,
    /** Usługa działa. */
    val running: Boolean = false,
    /** DynamicsProcessing dostępne (API 28+). */
    val supported: Boolean = true,
    /** Sesje zgłoszone przez odtwarzacze: id sesji → pakiet (jeśli podany). */
    val sessions: Map<Int, String?> = emptyMap(),
    /** Sesje, do których faktycznie podpięto efekt. */
    val attached: Set<Int> = emptySet(),
    val error: String? = null,
)

/**
 * Jedno źródło prawdy w procesie: parametry przetwarzania, stan odtwarzacza, sesje innych aplikacji
 * i aktywny silnik natywny. ViewModel pisze parametry, usługi je czytają.
 *
 * Synchronizacja z silnikiem: każda zmiana parametrów jest od razu przekazywana do aktywnego
 * [NativeEngine] (setery rdzenia to zapisy atomowe – tanie, bez blokowania wątku audio).
 * [engineLock] pilnuje, by podpięcie nowego silnika i równoległa zmiana parametrów nie zgubiły wartości.
 */
object AudioHub {

    private const val USER_PREFIX = "u_"

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val engineLock = Any()
    private var store: SettingsStore? = null

    private val _params = MutableStateFlow(EngineParams.fromPreset(Presets.default))
    val params: StateFlow<EngineParams> = _params.asStateFlow()

    private val _presetKey = MutableStateFlow(Presets.default.key)
    val presetKey: StateFlow<String> = _presetKey.asStateFlow()

    private val _player = MutableStateFlow(PlayerState())
    val player: StateFlow<PlayerState> = _player.asStateFlow()

    private val _global = MutableStateFlow(GlobalEffectsState())
    val global: StateFlow<GlobalEffectsState> = _global.asStateFlow()

    @Volatile
    private var activeEngine: NativeEngine? = null

    /** Wywoływane raz z `SzkloApp.onCreate`. */
    @OptIn(FlowPreview::class)
    fun init(context: Context) {
        if (store != null) return
        val s = SettingsStore(context.applicationContext)
        store = s
        val ps = UserPresetStore(context.applicationContext)
        presetStore = ps
        _userPresets.value = ps.load()
        spotifyWanted = s.loadSpotifyWanted()
        s.loadParams()?.let { _params.value = it }
        s.loadPresetKey()?.let { _presetKey.value = it }
        _global.update { it.copy(requested = s.loadGlobalRequested()) }

        // Zapis z opóźnieniem – przeciąganie suwaka nie pisze do pamięci co klatkę.
        scope.launch {
            combine(_params, _presetKey) { p, k -> p to k }
                .drop(1)
                .debounce(400)
                .collect { (p, k) -> s.save(p, k) }
        }
    }

    // ---------------------------------------------------------------- parametry

    fun updateParams(transform: (EngineParams) -> EngineParams) {
        synchronized(engineLock) {
            val old = _params.value
            val new = transform(old)
            if (new == old) return
            _params.value = new
            activeEngine?.let { applyDiff(it, old, new) }
        }
    }

    fun setPresetKey(key: String) {
        _presetKey.value = key
    }

    /** Natychmiastowy zapis bieżących ustawień (wyjście z aplikacji) – bez czekania na opóźniony zapis. */
    fun flush() {
        store?.save(_params.value, _presetKey.value, now = true)
    }

    // ---------------------------------------------------------------- presety użytkownika

    private var presetStore: UserPresetStore? = null
    private val _userPresets = MutableStateFlow<List<Preset>>(emptyList())
    val userPresets: StateFlow<List<Preset>> = _userPresets.asStateFlow()

    fun isUserPreset(key: String): Boolean = key.startsWith(USER_PREFIX)

    /** Preset wbudowany albo użytkownika. */
    fun presetByKey(key: String?): Preset? =
        Presets.byKey(key) ?: _userPresets.value.firstOrNull { it.key == key }

    private fun presetFromCurrent(key: String, name: String): Preset {
        val p = _params.value
        return Preset(key = key, name = name, master = p.master, modules = p.modules)
    }

    /**
     * „Zapisz jako”: bieżące ustawienia jako preset o nazwie [rawName]. Ta sama nazwa co istniejący preset
     * użytkownika = nadpisanie go; nazwy presetów wbudowanych są zajęte. Zwraca komunikat dla UI.
     */
    fun savePresetAs(rawName: String): String {
        val name = rawName.trim().take(40)
        if (name.isEmpty()) return "Podaj nazwę presetu"
        if (Presets.all.any { it.name.equals(name, ignoreCase = true) }) return "„$name” to preset wbudowany – wybierz inną nazwę"
        val list = _userPresets.value
        val existing = list.firstOrNull { it.name.equals(name, ignoreCase = true) }
        val key = existing?.key ?: (USER_PREFIX + System.currentTimeMillis().toString(36))
        val preset = presetFromCurrent(key, name)
        val updated = if (existing != null) list.map { if (it.key == key) preset else it } else list + preset
        presetStore?.saveAll(updated)
        _userPresets.value = updated
        _presetKey.value = key
        return if (existing != null) "Nadpisano „$name”" else "Zapisano „$name”"
    }

    /** „Zapisz”: nadpisuje bieżący preset użytkownika bieżącymi ustawieniami. */
    fun saveCurrentPreset(): String {
        val key = _presetKey.value
        val cur = _userPresets.value.firstOrNull { it.key == key }
            ?: return "Preset wbudowany – użyj „Zapisz jako”"
        val updated = _userPresets.value.map { if (it.key == key) presetFromCurrent(key, cur.name) else it }
        presetStore?.saveAll(updated)
        _userPresets.value = updated
        return "Zapisano „${cur.name}”"
    }

    /** „Usuń”: usuwa preset użytkownika. Bieżące ustawienia zostają (jako zmieniony preset domyślny). */
    fun deletePreset(key: String): String {
        val cur = _userPresets.value.firstOrNull { it.key == key } ?: return "Presetów wbudowanych nie można usunąć"
        val updated = _userPresets.value.filterNot { it.key == key }
        presetStore?.saveAll(updated)
        _userPresets.value = updated
        if (_presetKey.value == key) _presetKey.value = Presets.default.key
        return "Usunięto „${cur.name}”"
    }

    /** Czy bieżące ustawienia różnią się od wybranego presetu. */
    fun isModified(p: EngineParams, key: String): Boolean {
        val preset = presetByKey(key) ?: return true
        return !ParamsCodec.matches(p, preset)
    }

    // ---------------------------------------------------------------- silnik

    /** Podpina silnik odtwarzacza: wysyła pełny stan i od tej pory przekazuje każdą zmianę. */
    fun attachEngine(engine: NativeEngine) {
        synchronized(engineLock) {
            activeEngine = engine
            applyAll(engine, _params.value)
        }
    }

    /** Odpina silnik (przed jego zamknięciem). Nie odpina cudzego, jeśli w międzyczasie podpięto nowy. */
    fun detachEngine(engine: NativeEngine) {
        synchronized(engineLock) {
            if (activeEngine === engine) activeEngine = null
        }
    }

    private val meterBuf = FloatArray(NativeEngine.METER_COUNT)

    /** Odczyt mierników aktywnego silnika (null, gdy nic nie gra przez rdzeń). */
    fun readMeters(): MeterReading? {
        val e = activeEngine ?: return null
        if (_player.value.status != PlayerStatus.PLAYING) return null
        synchronized(meterBuf) {
            if (!e.readMeters(meterBuf)) return null
            return MeterReading(
                inDb = meterBuf[NativeEngine.METER_IN_DB],
                outDb = meterBuf[NativeEngine.METER_OUT_DB],
                grDb = meterBuf[NativeEngine.METER_GR_DB],
                lufs3s = meterBuf[NativeEngine.METER_LUFS_3S],
                truePeakDb = meterBuf[NativeEngine.METER_TRUE_PEAK_DB],
            )
        }
    }

    private fun applyAll(e: NativeEngine, p: EngineParams) {
        p.modules.forEachIndexed { i, m -> e.setModule(i, m.on, m.amount) }
        e.setMaster(p.master)
        e.setEnabled(p.enabled)
        e.setCompare(p.compare)
        e.setCeiling(p.ceilingDb)
    }

    private fun applyDiff(e: NativeEngine, old: EngineParams, new: EngineParams) {
        for (i in 0 until ModuleId.COUNT) {
            val m = new.modules[i]
            if (m != old.modules[i]) e.setModule(i, m.on, m.amount)
        }
        if (new.master != old.master) e.setMaster(new.master)
        if (new.enabled != old.enabled) e.setEnabled(new.enabled)
        if (new.compare != old.compare) e.setCompare(new.compare)
        if (new.ceilingDb != old.ceilingDb) e.setCeiling(new.ceilingDb)
    }

    // ---------------------------------------------------------------- odtwarzacz

    fun updatePlayer(transform: (PlayerState) -> PlayerState) = _player.update(transform)

    // ---------------------------------------------------------------- Spotify Connect

    /** Odbiornik był włączony przez użytkownika – przy następnym starcie aplikacji włącza się sam. */
    @Volatile
    var spotifyWanted: Boolean = false
        private set

    fun setSpotifyWanted(on: Boolean) {
        spotifyWanted = on
        store?.saveSpotifyWanted(on)
    }

    private val _spotify = MutableStateFlow(SpotifyState())
    val spotify: StateFlow<SpotifyState> = _spotify.asStateFlow()
    fun updateSpotify(transform: (SpotifyState) -> SpotifyState) = _spotify.update(transform)

    // ---------------------------------------------------------------- inne aplikacje

    fun setGlobalRequested(on: Boolean) {
        _global.update { it.copy(requested = on, error = if (on) it.error else null) }
        store?.saveGlobalRequested(on)
    }

    fun updateGlobal(transform: (GlobalEffectsState) -> GlobalEffectsState) = _global.update(transform)

    /** Odtwarzacz zgłosił sesję (OPEN_AUDIO_EFFECT_CONTROL_SESSION lub panel korektora). */
    fun addSession(sessionId: Int, packageName: String?) {
        if (sessionId <= 0) return // 0 = mikser globalny – przestarzałe, nie podpinamy
        _global.update { it.copy(sessions = it.sessions + (sessionId to packageName)) }
    }

    fun removeSession(sessionId: Int) {
        _global.update { it.copy(sessions = it.sessions - sessionId, attached = it.attached - sessionId) }
    }
}
