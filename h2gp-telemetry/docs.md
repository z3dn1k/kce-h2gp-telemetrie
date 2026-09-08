# Telemetrický systém H2GP – Technická dokumentace

Tento dokument slouží jako komplexní technická referenční příručka telemetrického desktopového systému vyvinutého v programovacím jazyce **Rust** pro závodní vodíkový automobil soutěže **H2GP (Horizon Hydrogen Grand Prix)**.

Dokumentace detailně popisuje architekturu, komunikační protokol, správu dat v paměti, konfiguraci, perzistenci a bezpečnostní mechanismy implementované k současnému datu.

---

## 1. Architektura systému a model souběžnosti

Systém je koncipován jako vysoce spolehlivá asynchronní desktopová aplikace s odděleným zpracováním I/O operací a vykreslování uživatelského rozhraní. Cílem této architektury je zaručit, že blokující operace sériové linky nebo diskového I/O nikdy nezpůsobí pokles snímkové frekvence (FPS) ani zamrznutí grafického rozhraní.

### 1.1 Model více vláken (Threading Model)
Aplikace běží na modelu rozdělených vláken propojených pomocí asynchronních typově bezpečných front:

```
┌─────────────────────────┐               ┌─────────────────────────┐
│     Sériové vlákno      │               │       Demo vlákno       │
│      (serial.rs)        │               │        (demo.rs)        │
│  - čtení USB portu      │               │  - offline CSV replay   │
│  - synchronizace rámců  │               │  - syntetické hodiny    │
│  - dekódování binárky   │               └────────────┬────────────┘
└────────────┬────────────┘                            │
             │           mpsc::Sender<TelemetrySample> │
             └───────────────────┬─────────────────────┘
                                 │
                                 ▼ mpsc::Receiver<TelemetrySample>
                     ┌─────────────────────────┐
                     │       Vlákno UI         │
                     │    (main.rs + ui.rs)    │
                     │  - egui/eframe smyčka   │
                     │  - filtrace anomálií    │
                     │  - správa DataManageru  │
                     │  - vykreslování grafů   │
                     └───────────┬─────────────┘
                                 │ mpsc::Sender<String>
                                 ▼ (JSON příkazy)
                     ┌─────────────────────────┐
                     │     Sériový zápis       │
                     │     (MCU uplink)        │
                     └─────────────────────────┘
```

1. **Vlákno uživatelského rozhraní (UI Thread)**:
   - Zajišťuje běh grafického okna pomocí frameworku `eframe` / `egui` v okamžitém režimu (*immediate-mode GUI*).
   - Obnovovací frekvence je řízena intervalem `ui_refresh_interval_ms` (výchozí 33 ms ≈ 30 FPS).
   - Na začátku každého snímku vyprázdní frontu `rx.try_recv()` ve smyčce `while let Ok(sample)`, zpracuje došlé vzorky, zkontroluje anomálie, aktualizuje datový manažer a provede překreslení.
2. **Vlákno sériové linky (Serial Thread)**:
   - Běží na pozadí v odděleném vlákně operačního systému (`std::thread::spawn`).
   - Otevírá USB virtuální sériový port s krátkým timeoutem (20 ms) a kontinuálně načítá surové bajty do akumulačního bufferu o kapacitě 4096 bajtů.
   - Vyhledává synchronizační sekvence, parsuje binární rámce a odesílá zkonstruované instance `TelemetrySample` skrze kanál `mpsc::Sender`.
   - Zároveň neblokujícím způsobem monitoruje kanál `cmd_rx` pro odesílání řídicích JSON příkazů zpět do MCU vozidla (např. řízení ventilátoru či změna kódu jezdce).
3. **Vlákno simulace (Demo Thread)**:
   - Slouží k plnohodnotnému offline testování bez připojeného hardware.
   - Načítá záznam `data.csv`, ignoruje historická nerovnoměrná časová razítka a generuje stabilní proud vzorků s frekvencí 10 Hz (interval 100 ms) se syntetickými monotónními hodinami.

---

## 2. Binární komunikační protokol (`protocol.rs`)

Z důvodu minimalizace režie přenosu a latence byl textový formát (např. ASCII CSV po lince) nahrazen binárním rámcovým protokolem s pevnou délkou a synchronizačními znaky.

### 2.1 Struktura rámce (Frame Framing)
Každý rámec na sběrnici začíná 10bajtovou hlavičkou:

| Bajtový offset | Datový typ | Název pole | Význam |
|---|---|---|---|
| `0..4` | `[u8; 4]` | `MAGIC` | Synchronizační ASCII sekvence: `0x48, 0x32, 0x47, 0x50` ("H2GP") |
| `4` | `u8` | `kind` | Identifikátor typu paketu: `1` = Hlavní telemetrie, `42` = Pomocná telemetrie (AUX) |
| `5` | `u8` | `reserved` | Rezervováno pro budoucí rozšíření / flags |
| `6..8` | `u16` (LE) | `payload_len` | Délka užitečného zatížení (payloadu) v bajtech (např. 88 B pro hlavní paket) |
| `8..10` | `u16` (LE) | `crc / seq` | Kontrolní součet nebo sekvenční číslo |

Pokud parser v bufferu narazí na poškozená data, posune se na nejbližší výskyt sekvence `MAGIC`. Bajty před tímto výskytem jsou bezpečně zahozeny (`drain`).

### 2.2 Dekódování senzorů INA228 (`decode_ina_channel`)
Hlavní telemetrický paket nese dva 28bajtové bloky registrů digitálního monitoru výkonu **Texas Instruments INA228**:
1. Kanál akumulátoru / kapacitní banky (`batt` - BATT/CBANK) na offsetu `16..44`
2. Kanál palivového článku (`fc` - FUEL CELL) na offsetu `44..72`

#### Rozložení 28bajtového registrového bloku INA228:
| Offset v bloku | Velikost | Formát v INA228 | Fyzikální veličina |
|---|---|---|---|
| `0..4` | 4 bajty | 20-bit se znaménkem (posun o 4 doprava) | Napětí bočníku ($V_{shunt}$) |
| `4..8` | 4 bajty | 24-bit bez znaménka | Napětí sběrnice ($V_{bus}$) |
| `8..10` | 2 bajty | 16-bit se znaménkem | Vnitřní teplota čipu ($T_{die}$) |
| `10..14` | 4 bajty | 20-bit se znaménkem (posun o 4 doprava) | Proud ($I$) |
| `14..18` | 4 bajty | 24-bit bez znaménka | Výkon ($P$) |
| `18..23` | 5 bajtů | 40-bit bez znaménka | Kumulovaná energie ($E$) |
| `23..28` | 5 bajtů | 40-bit se znaménkem | Kumulovaný náboj ($Q$) |

#### Fyzikální konstanty a bitové operace (LSB):
Převod surových celočíselných registrů na fyzikální hodnoty s plovoucí řádovou čárkou (`f64`) se provádí pomocí přesně kalibrovaných konstant:

- **Znaménková rozšíření**:
  - `sign_extend_20(raw >> 4)`: Převede 20bitové dvojkový doplněk na 32bitové celé číslo se znaménkem (`i32`) pomocí bezztrátových bitových posunů: `((val << 12) as i32) >> 12`.
  - `sign_extend_40(raw)`: Převede 40bitový registr náboje na 64bitové celé číslo se znaménkem (`i64`): `((val << 24) as i64) >> 24`.
- **Proud ($I$)**:
  $$I = \text{raw}_{20} \times \frac{32.0}{524288.0}\text{ [A]}$$
- **Napětí sběrnice ($V$)**:
  $$V = \text{raw}_{24} \times 0.0001953125\text{ [V]}$$
- **Napětí bočníku ($V_{shunt}$)**:
  $$V_{shunt} = \text{raw}_{20} \times 0.0003125 \times 1000.0\text{ [mV]}$$
- **Teplota čipu ($T$)**:
  $$T = \text{raw}_{16} \times \frac{1.0}{128.0}\text{ [}^\circ\text{C]}$$
- **Výkon ($P$)**:
  $$P = \text{raw}_{24} \times (3.2 \times I_{LSB})\text{ [W]}$$
- **Energie ($E$)**:
  $$E = \text{raw}_{40} \times (16.0 \times P_{LSB})\text{ [J]}$$
- **Kapacita / Náboj ($Ah$)**:
  $$Ah = \frac{\text{raw}_{40} \times I_{LSB}}{3600.0}\text{ [Ah]}$$

### 2.3 Dekódování pomocné telemetrie (`decode_rev3_aux`)
Paket typu `TELEMETRY_KIND_AUX` (typ 42) nese diagnostická data výkonové desky Rev3:
- Až 4 teplotní čidla (DS18B20 / NTC) přepočtená vzorcem $T = \text{raw} / 16.0\,^\circ\text{C}$.
- Ošetření neplatného stavu: hodnota `i16::MIN` značí odpojené čidlo a je bezpečně mapována na $-127.0\,^\circ\text{C}$.
- Maximální teplota z aktivních čidel.
- PWM střída ventilátoru (0–100 %).
- Stavové příznaky `flags`:
  - `Bit 0` (`0x01`): Zkratování palivového článku (aktivní kondicionování článku).
  - `Bit 1` (`0x02`): Proplach vodíku (Purge ventil otevřen).
- Režim ventilátoru: `0 = Auto`, `1 = Manual`, `2 = Off`.

---

## 3. Správa dat a paměťový kruhový buffer (`datamanager.rs`)

Třída `DataManager` slouží jako centrální in-memory úložiště telemetrických vzorků pro vykreslovací komponenty.

### 3.1 Dávkový kruhový buffer (Batch-draining Buffer)
Běžné kruhové buffery založené na odstraňování jednoho prvku při každém vložení (`history.remove(0)`) vyžadují posun celého pole v paměti ($O(N)$ operace), což při vysokých frekvencích vzorkování zbytečně zatěžuje CPU a alokátor.

`DataManager` implementuje optimalizované dávkové odstraňování:
- Alokuje kapacitu pro `max_history + batch_size`, kde `batch_size = clamp(max_history / 10, 10, 500)`.
- Vzorky jsou přidávány v čase $O(1)$ přes `Vec::push`.
- Teprve když délka pole dosáhne hranice `max_history + batch_size`, provede se jednorázové odříznutí nadbytečných vzorků pomocí `history.drain(0..excess)`.
- Režie posunu paměti je tak amortizována a probíhá pouze jednou za desítky či stovky vzorků.

### 3.2 Ochrana před zpětným časovým skokem
Pokud dojde k restartu mikrokontroléru na autě nebo k opětovnému spuštění demo smyčky, časové razítko nového vzorku je menší než časové razítko předchozího vzorku. Pokud by se tato data vykreslila do spojitého grafu, spojnice by se vrátila v čase zpět a vytvořila nečitelný grafický artefakt ("Z-fold spaghetti").
`DataManager::add_data` monitoruje monotónnost:
```rust
if let Some(&last_time) = self.time_labels.last() {
    if elapsed_seconds < last_time {
        self.clear();
    }
}
```
Při zjištění skoku se buffer automaticky vyčistí a graf pokračuje plynule od nuly.

### 3.3 Extrémy a statistické průměry
- **Průběžná minima a maxima**: Pro $V$, $I$, $P$ na obou kanálech jsou udržovány hodnoty `(f64, f64)`. Při vložení vzorku se aktualizují v čase $O(1)$. Po dávkovém oříznutí bufferu se provede jednorázový přepočet z aktuálních prvků v paměti.
- **Klouzavý průměr (SMA)**: Metody `compute_batt_voltage_avg`, `compute_fc_voltage_avg`, atd. počítají aritmetický průměr za posledních $N$ vzorků. Index počátku okna je ošetřen pomocí `len.saturating_sub(window)`, což eliminuje riziko podtečení celočíselného typu.

### 3.4 Export závěrečného reportu (`export_summary`)
Metoda vygeneruje strukturovaný textový soubor `summary.txt` obsahující celkovou spotřebovanou energii z baterie i palivového článku (J), špičkové proudy (A), maximální naměřené teploty (°C) a celkový počet zpracovaných paketů.

---

## 4. Centrální konfigurace aplikace (`config.rs`, `config.toml`)

Veškeré hardwarové konstanty, prahové hodnoty anomálií a časovací konstanty jsou sjednoceny ve struktuře `AppConfig`. Aplikace podporuje čtení a zápis konfiguračního souboru ve formátu **TOML** bez nutnosti rekompilace binárního souboru.

### 4.1 Přehled konfiguračních parametrů:
| Parametr | Datový typ | Výchozí hodnota | Popis |
|---|---|---|---|
| `serial_baud_rate` | `u32` | `115200` | Přenosová rychlost sériového portu |
| `default_port` | `String` | `"COM3"` | Výchozí název portu |
| `buffer_capacity` | `usize` | `1200` | Maximální počet vzorků v grafu |
| `sma_window` | `usize` | `10` | Velikost okna pro klouzavý průměr |
| `anomaly_batt_overcurrent_a` | `f64` | `15.0` | Proudový limit baterie (A) |
| `anomaly_fc_vsag_v` | `f64` | `9.0` | Detekce poklesu napětí článku (V) |
| `anomaly_fc_min_v` | `f64` | `2.0` | Spodní mez pro rozlišení odpojení (V) |
| `anomaly_batt_overtemp_c` | `f64` | `45.0` | Teplotní limit baterie (°C) |
| `anomaly_queue_capacity` | `usize` | `10` | Počet položek v bočním panelu UI |
| `ui_refresh_interval_ms` | `u64` | `33` | Perioda překreslování UI (~30 FPS) |
| `default_chart_window_s` | `f64` | `30.0` | Výchozí časové okno grafu (s) |
| `default_driver_code` | `String` | `"SKL"` | Kód jezdce pro LED matici |
| `default_fan_duty` | `i32` | `70` | Výchozí střída ventilátoru (%) |
| `demo_interval_ms` | `u64` | `100` | Perioda přehrávání demo režimu (10 Hz) |

### 4.2 Bezpečné načítání (`load_or_default`)
- Pokud soubor `config.toml` existuje a je validní, načte se přes `toml::from_str`.
- Pokud soubor neexistuje, aplikace automaticky vygeneruje nový soubor `config.toml` s výchozími hodnotami a pokračuje v běhu.
- Pokud soubor obsahuje syntaktickou chybu, aplikace vypíše varování do chybového výstupu a bezpečně použije vestavěné výchozí hodnoty, aniž by došlo k pádu programu.

---

## 5. Zpracování chyb a typová bezpečnost

Projekt důsledně dodržuje idiom jazyka Rust eliminující runtime paniky (`unwrap()`) v produkčních cestách kódu.

### 5.1 Centralizovaný chybový typ (`error.rs`)
Pomocí knihovny `thiserror` je definován výčet `TelemetryError`, kde každá varianta nese přesný kontext:
```rust
#[derive(Debug, Error)]
pub enum TelemetryError {
    #[error("Serial port '{port}' could not be opened at {baud} baud: {source}")]
    SerialOpen { port: String, baud: u32, source: serialport::Error },

    #[error("Serial port '{port}' could not be cloned for write access: {source}")]
    SerialClone { port: String, source: serialport::Error },

    #[error("UI channel disconnected — receiver was dropped")]
    ChannelDisconnected,

    #[error("Demo file '{path}' could not be opened: {source}")]
    DemoFileOpen { path: String, source: std::io::Error },

    #[error("Demo file '{path}' contains no data rows")]
    DemoFileEmpty { path: String },

    #[error("Report export failed: {0}")]
    ReportExport(#[from] std::io::Error),
}
```
Vlákna na pozadí (`run_serial_loop`, `run_demo_loop`) vracejí `Result<(), TelemetryError>`. Chyby jsou propagovány pomocí operátoru `?` a při odpojení přijímače v UI se vlákna korektně ukončí namísto nekonečného cyklení.

### 5.2 Typově bezpečný režim ventilátoru (`FanMode`)
Místo původního řetězce `String`, který mohl obsahovat překlepy, je použit přísný výčet:
```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FanMode {
    Auto,
    Manual,
    Off,
}
```
- Implementuje `Display` pro popisky v UI (`"AUTO"`, `"MANUAL"`, `"OFF"`).
- Poskytuje metodu `.as_str()`, která vrací přesný malý řetězec vyžadovaný protokolem mikrokontroléru (`"auto"`, `"manual"`, `"off"`).
- Výběr v rozhraní a generování JSON příkazu probíhá přes vyčerpávající `match`, což znemožňuje vznik neplatného příkazu.

---

---

## 6. Vláknově bezpečné protokolování (`logger.rs`) a optimalizace toku dat

Perzistence telemetrických dat probíhá do dvou souborů:
1. `data.csv`: Kompletní proud měření obsahující časové razítko a všech 14 elektrických i teplotních veličin.
2. `anomalies.log`: Záznam detekovaných anomálií ve sloupcovém formátu s pevnými šířkami (čas, hodnota, možná příčina).

### 6.1 Eliminace duplicitního zápisu (Single Logging Point)
Původní verze kódu volala `logger::append_to_csv` souběžně v `serial.rs` i `main.rs`, což vedlo ke zdvojení každého záznamu na disku. Zápis byl konsolidován výhradně do `serial.rs`:
- Na disk se ukládají pouze reálná data přijatá z hardwaru auta.
- Režim simulace (`demo.rs`) nepropisuje simulovaná data zpět do `data.csv`, čímž se předchází nekonečnému cyklickému nafukování logovacího souboru.
- Diskové I/O operace jsou zcela odkloněny z vykreslovacího UI vlákna do vlákna sériové linky, čímž je chráněna plynulost grafického rozhraní.

### 6.2 Architektura loggeru
- Souborové deskriptory jsou uloženy v globálním registru `OnceLock<Mutex<HashMap<String, LogWriter>>>`.
- Každý soubor má vlastní zámek `Arc<Mutex<BufWriter<File>>>`. Tím je zaručeno, že zápis do CSV neblokuje souběžný zápis anomálie do logu.
- Využívá vyrovnávací paměť o velikosti 8192 bajtů (`BufWriter`), což snižuje počet systémových volání operačního systému na minimum.
- Funkce `get_or_create_writer` používá bezpečné zachycení chyb; při selhání otevření souboru nezpůsobí pád aplikace, ale pouze zaloguje chybu do konzole.

---

## 7. Samostatný modul detekce anomálií (`anomaly.rs`)

Detekce rizikových stavů byla zcela oddělena z UI smyčky do dedikovaného modulu `anomaly.rs`. Tím je zajištěna čistá architektura, snadná rozšiřitelnost a možnost nezávislého unit testování.

### 7.1 Struktura události anomálie
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnomalyKind {
    BattOvercurrent,
    FcVoltageSag,
    BattOvertemp,
    FcShort,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Anomaly {
    pub timestamp_ms: u32,
    pub kind: AnomalyKind,
    pub value_str: String,
    pub cause: &'static str,
}
```

### 7.2 Pravidla detekce anomálií
Při příchodu každého paketu vyhodnocuje funkce `detect_anomalies(&sample, &config)` kritické provozní stavy:
1. **Nadproud baterie (`BATT OVERCURRENT`)**:
   - Podmínka: $I_{batt} > \text{config.anomaly\_batt\_overcurrent\_a}$ (výchozí 15.0 A).
   - Diagnostika: Zaseknutí serva řízení nebo zkrat na trakčním měniči.
2. **Pokles napětí palivového článku (`FC V-SAG`)**:
   - Podmínka: $\text{config.anomaly\_fc\_min\_v} < V_{fc} < \text{config.anomaly\_fc\_vsag\_v}$ (výchozí 2.0 V až 9.0 V).
   - Diagnostika: Vyčerpání vodíku v okruhu (starvation) nebo zanesení membrány vodou. Spodní mez 2.0 V rozlišuje pokles napětí od odpojení článku.
3. **Přehřátí baterie (`BATT OVERTEMP`)**:
   - Podmínka: $T_{batt} > \text{config.anomaly\_batt\_overtemp\_c}$ (výchozí 45.0 °C).
   - Diagnostika: Nadměrné zatížení či vysoký vnitřní odpor článků.
4. **Zkratování článku (`FC SHORT`)**:
   - Podmínka: Příznak v pomocných datech `flags & 0x01 != 0`.
   - Diagnostika: Řídicí jednotka aktivně zkratuje článek pro obnovu katalyzátoru.

Všechny detekované anomálie jsou současně zapsány na disk do `anomalies.log` a vloženy do ohraničené fronty `VecDeque` v UI (max. 10 položek), kde se zobrazují v reálném čase v bočním panelu.

---

## 8. Řízení životního cyklu a stavu spojení (`ConnectionState`)

### 8.1 Vícestavový automat spojení
Místo pouhého binárního příznaku `bool` využívá aplikace stavový automat:
```rust
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Live { since: Instant },
    DemoMode { since: Instant },
    Error(String),
}
```
Metoda `.status_text()` poskytuje barevný badge a živý čítač doby trvání relace (např. `● LIVE [02:45]` nebo `● DEMO [01:10]`), což umožňuje pit-crew týmu okamžitý přehled o době od startu spojení.

### 8.2 Čisté ukončení vláken (Graceful Shutdown)
- Pro řízení vláken na pozadí je použit atomický příznak `Arc<AtomicBool>`.
- Spuštěná vlákna (`run_serial_loop`, `run_demo_loop`) v každé iteraci kontrolují `is_running.load(Ordering::Relaxed)`.
- Metoda `TelemetryApp::stop_worker()` umožňuje kdykoliv bezpečně zastavit předchozí vlákno při přepnutí režimu nebo kliknutí na **DISCONNECT**.
- Implementace rozhraní `Drop for TelemetryApp` zaručuje, že při zavření okna aplikace se všechna vlákna na pozadí korektně ukončí bez osiření procesů.

---

## 9. Testování a kontrola kvality kódu

Projekt obsahuje rozsáhlou sadu automatizovaných unit testů v souborech `protocol.rs`, `datamanager.rs`, `config.rs` a `anomaly.rs`.

### Přehled testovací sady:
- **`protocol::tests` (22 testů)**:
  - Znaménková rozšíření 20bitových a 40bitových čísel na kladných, záporných i hraničních hodnotách.
  - Ochrana před podtečením a přetečením bufferu při zkrácených a prázdných vstupech.
  - Přesnost dekódování registrů INA228 oproti teoretickým LSB konstantám.
  - Zpracování speciálních hodnot (např. sentinel $-127.0\,^\circ\text{C}$ pro odpojené teplotní čidlo).
- **`datamanager::tests` (7 testů)**:
  - Inicializace a chování na prázdném bufferu.
  - Správnost časových razítek a minima/maxima.
  - Detekce zpětného skoku a vyčištění paměti.
  - Výpočty klouzavých průměrů pro různá okna.
  - Dávkové odříznutí bufferu a přepočet extrémů.
  - Export reportu bez selhání.
- **`config::tests` (4 testy)**:
  - Shoda výchozích konfiguračních hodnot.
  - Round-trip serializace a deserializace formátu TOML.
  - Zotavení z neplatného TOML souboru.
  - Uložení a načtení konfigurace z disku.
- **`anomaly::tests` (6 testů)**:
  - Detekce nadproudu baterie a ignorování bezpečných proudů.
  - Detekce poklesu napětí palivového článku a ignorování odpojeného článku (< 2.0 V).
  - Detekce přehřátí baterie.
  - Detekce zkratovacího příznaku v pomocné telemetrii.
- **`serial::tests` (1 test)**:
  - Bezpečná auto-detekce sériových portů v systému bez rizika pádu či paniky.
- **`main::tests` (5 testů)**:
  - Funkčnost vícestavového automatu spojení `ConnectionState::is_connected`.
  - Formátování textu stavu a stopek doby spojení (`status_text`).
  - Přepínání stavu pozastavení grafu (`toggle_pause`).
  - Čistý reset metriky PPS a stavu při odpojení (`disconnect`).
  - Ohraničená FIFO kapacita fronty anomálií a správná rotace prvků.

**Výsledek verifikace**: Všech **45 testů** prochází úspěšně. Nástroj `cargo clippy` hlásí **0 varování** a generování HTML dokumentace `cargo doc --no-deps` probíhá s **0 chybami**.

---

## 10. Pokročilé funkce pro obsluhu a vizualizaci (Fáze 8 — Cool Features)

Pro zajištění špičkové ergonomie pit-crew týmu a demonstraci pokročilých schopností grafického subsystému `egui` a jazyka Rust byly implementovány následující funkce:

### 10.1 Automatická detekce sériových portů (`detect_available_ports`)
Namísto nutnosti manuálního zadávání systémových identifikátorů (např. `COM3` ve Windows nebo `/dev/ttyUSB0` v Linuxu) aplikace automaticky prohledává systém:
- Funkce `serial::detect_available_ports()` využívá `serialport::available_ports()`.
- Pokud jsou nalezeny aktivní porty, v horním panelu nástrojů se zobrazí interaktivní `egui::ComboBox` s dostupnými zařízeními.
- Uživatel má k dispozici tlačítko `↻` pro okamžité opětovné proskenování sběrnice (např. po zapojení nového USB převodníku za běhu aplikace).
- Pokud žádný port není detekován, rozhraní inteligentně přepne na přímé textové editační pole.

### 10.2 Klávesové zkratky a ergonomie řízení
Během závodu v boxech je manipulace s myší často nepraktická. Aplikace plně podporuje ovládání pomocí klávesnice:
- `1`, `2`, `3`, `4`: Okamžité přepínání zobrazeného telemetrického kanálu v grafu (`1: NAPĚTÍ`, `2: PROUD`, `3: VÝKON`, `4: ENERGIE`).
- `Mezerník` (Space): Pozastavení nebo obnovení toku dat v grafu (`Pause / Resume`).
- `C`: Připojení k vybranému sériovému portu nebo odpojení aktivní relace.
- `D`: Spuštění offline demo simulace z `data.csv`.

**Ochrana kontextu vstupu**: Klávesové zkratky jsou vyhodnocovány s podmínkou `if !ctx.wants_keyboard_input()`. Pokud operátor právě edituje textové pole (např. zadává kód jezdce `driver_code` nebo název portu), stisk kláves se interpretuje jako běžný text a nespustí nechtěně povel k odpojení či přepnutí grafu.

### 10.3 Pozastavení toku dat (Freeze / Pause) pro telemetrickou analýzu
Stiskem mezerníku nebo tlačítka v horní liště může telemetrický inženýr kdykoliv graf pozastavit:
- V pozastaveném stavu se vzorky nepřidávají do `DataManageru`, takže křivka grafu "zamrzne" a neodjíždí z obrazovky.
- Inženýr může detailně prozkoumat nedávný pokles napětí, proudovou špičku či přechodový děj.
- V UI svítí výrazný zlatý indikátor `⏸ POZASTAVENO [Mezerník]`.
- Opětovný stisk mezerníku okamžitě obnoví živé vzorkování.

### 10.4 Měření kvality spojení (PPS — Packets Per Second)
Pro okamžitý přehled o spolehlivosti rádiového / USB spojení počítá aplikace v reálném čase frekvenci přijímaných paketů:
- Každou sekundu se vyhodnotí počet příchozích vzorků vztažený k uplynulému času:
  $$\text{PPS} = \frac{\text{packet\_counter}}{\Delta t}$$
- Hodnota je zobrazena v horní liště s dynamickým barevným kódováním:
  - **Zelená (≥ 10 PPS)**: Vynikající propustnost a stabilní tok dat.
  - **Oranžová (1–9 PPS)**: Zpomalený tok dat nebo vyšší chybovost na lince.
  - **Červená (0 PPS)**: Výpadek příjmu dat při aktivním spojení.

### 10.5 Export snímků obrazovky do PNG (Screenshot Export)
Aplikace obsahuje integrovanou funkci pro pořizování grafických reportů:
- Tlačítko `📷 SCREENSHOT` odešle frameworku příkaz `ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()))`.
- V následujícím rámci aplikace zachytí vygenerovanou událost `egui::Event::Screenshot`, která nese surový barevný buffer `Arc<egui::ColorImage>`.
- Pixely jsou převedeny do barevného formátu RGBA a uloženy na disk pomocí knihovny `image::RgbaImage` jako `screenshot_<timestamp>.png`.
- V horní liště se po úspěšném uložení zobrazí přívětivá zelená notifikace (toast) informující uživatele o přesném názvu vytvořeného souboru.

### 10.6 Sledování doby aktivního spojení (Uptime)
Stavový indikátor `ConnectionState` průběžně počítá a formátuje uplynulý čas od navázání spojení ve formátu `[MM:SS]`. Posádka v boxech tak má přesný přehled o tom, jak dlouho probíhá aktuální stint nebo měření.

---

## 11. Projektová metadata, bezpečnostní politiky a distribuce (Fáze 9)

### 11.1 Metadata balíčku v manifestu `Cargo.toml`
Konfigurační soubor `Cargo.toml` byl povýšen na standard vyžadovaný ekosystémem `crates.io` a nástroji pro audit kódu:
- **Identifikace projektu**: Název balíčku `h2gp-telemetry`, verze `1.0.2`, edice `2021`.
- **Autorské a licenční údaje**: Autor `Jan Zedník`, licence `MIT`, odkaz na veřejný repozitář na platformě GitHub.
- **Kategorizace a klíčová slova**: Tagy `telemetry`, `h2gp`, `fuel-cell`, `egui`, `serial` usnadňující indexaci a audit závislostí.
- **Specifikace dokumentace**: Přímý odkaz na `README.md` pro zobrazení úvodní stránky repozitáře a generované dokumentace balíčku.

### 11.2 Zákaz nebezpečného kódu (`forbid(unsafe_code)`)
V manifestu `Cargo.toml` je aktivována přísná bezpečnostní direktiva kompilátoru:
```toml
[lints.rust]
unsafe_code = "forbid"
```
Tato konfigurace na úrovni celého crate garantuje:
- V žádné části zdrojových kódů nesmí být použit klíčový blok `unsafe`.
- Pokud by se kdokoliv pokusil obejít typový systém Rustu (např. surovými ukazateli nebo nedefinovaným chováním), kompilátor okamžitě odmítne sestavení jako fatální chybu.
- Pro účely obhajoby maturitní práce to slouží jako hmatatelný důkaz porozumění bezpečnostnímu modelu paměti (*Memory Safety*).

### 11.3 Repozitářová dokumentace `README.md`
Repozitář byl vybaven kompletní průvodní dokumentací obsahující:
- Architektonický diagram toku dat a vláknového modelu.
- Specifikaci binárního komunikačního rámce a přepočtů registrů INA228.
- Technické odůvodnění výběru jednotlivých crate (`eframe`, `serialport`, `thiserror`, `toml`, `image`).
- Přehlednou tabulku klávesových zkratek.
- Návod pro spuštění v demo režimu a spuštění kompletní verifikační sady testů.

