package pl.szklo

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

class DziennikTest {

    @Test
    fun accountNameIsMasked() {
        val l = Dziennik.zamaz("11-24 13:00:00.000 I SzkloSpotify: librespot_core::session: Authenticated as 'jan.kowalski' !")
        assertFalse(l, l.contains("kowalski"))
        assertEquals("I x: zalogowano (***)", Dziennik.zamaz("I x: zalogowano (jan)"))
    }

    @Test
    fun sessionEventAndLoginAddressAreMasked() {
        val e = Dziennik.zamaz("I x: zdarzenie: SessionConnected { connection_id: \"ab\", user_name: \"jan\" }")
        assertFalse(e, e.contains("jan"))
        val u = Dziennik.zamaz("I x: Spotify: login https://accounts.spotify.com/authorize?client_id=1&state=tajne")
        assertEquals("I x: Spotify: login https://accounts.spotify.com/authorize?***", u)
    }

    @Test
    fun ordinaryLinesStayUntouched() {
        val l = "I SzkloSpotify: odtwarzacz: Loading <Wicked Game> with Spotify URI <spotify:track:1>"
        assertEquals(l, Dziennik.zamaz(l))
    }
}
