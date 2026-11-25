//! Spotify w Szkle: biblioteka użytkownika (polubione utwory, playlisty z folderami, albumy, wykonawcy, podcasty) oraz
//! wyszukiwanie w zaznaczonej liście, w całej bibliotece i w katalogu Spotify. Wspólne dla programu na PC i Androida.
//!
//! Dane idą przez librespot (otwarty, nieoficjalny klient Spotify – MIT), na sesji, którą użytkownik sam zalogował.
//! Tylko odczyt: niczego w bibliotece Spotify nie zmieniamy.
//!
//! Spotify to znak towarowy Spotify AB; Szkło nie jest powiązane ze Spotify.

mod dane;
mod kontroler;
pub mod tekst;

pub use dane::{biblioteka, katalog, tresc, Album, Biblioteka, Lista, Odn, Pamiec, Tresc, Utwor, Wykonawca, Wyniki, ALBUMY, POLUBIONE, WYKONAWCY};
pub use kontroler::{szukaj_w, Kontroler};
