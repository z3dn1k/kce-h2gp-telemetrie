# H2GP Telemetrie – Jednoduchý průvodce pro tebe k obhajobě

Tento dokument je napsaný **jednoduše a lidsky**, abys přesně chápal, co který řádek v projektu dělá, proč je to tak napsané a co odpovědět učiteli nebo komisi u maturitní zkoušky.

---

## 1. Co tenhle projekt vlastně dělá?

Představ si, že závodíme s malým autem na vodík v soutěži **H2GP**. V autě jsou dva zdroje energie:
1. **Vodíkový palivový článek (Fuel Cell)** – vyrábí elektřinu z vodíku.
2. **Akumulátor / superkondenzátory (BATT/CBANK)** – pomáhají při akceleraci a ukládají přebytky.

Na autě je mikrokontrolér (řídicí deska se senzory proudu a napětí INA228). Přes USB kabel posílá do počítače v boxech desítky čísel za sekundu. 

Tato naše aplikace v Rustu:
- Čte tato data z USB portu v reálném čase.
- Okamžitě je přepočítává na volty, ampéry, watty, jouly a stupně Celsia.
- Kreslí z nich plynulé živé grafy a ukazuje aktuální stav (jestli se baterie nabíjí nebo vybíjí).
- Hlídá průšvihy (anomálie) – např. když autu dochází vodík nebo když se zaseklo servo řízení a teče moc proudu.
- Ukládá všechno na disk do CSV a umí vygenerovat závěrečný report ze závodu.
- Umožňuje z počítače poslat příkaz zpátky do auta (např. nastavit ventilátor nebo změnit kód jezdce na displeji).
- Má **Demo režim**, takže si celou aplikaci můžeš pustit a ukázat komisi i bez fyzického auta!

---

## 2. Mapa souborů (Kde co najdeš)

| Soubor | Co má na starosti jednoduše |
|---|---|
| `main.rs` | Hlavní startér programu, drží stav aplikace a spouští okno. |
| `ui.rs` | Vzhled aplikace – tlačítka, budíky, barevné sloupce, grafy, posuvníky. |
| `protocol.rs` | "Překladač" – bere surové bajty z auta a dělá z nich čísla (volty, ampéry). |
| `datamanager.rs` | "Paměť grafu" – ukládá posledních 1200 vzorků a počítá průměry a extrémy. |
| `config.rs` | Nastavení – načítá soubor `config.toml` (limity, rychlost, barvy). |
| `anomaly.rs` | Hlídač anomálií – kontroluje limity (proudy, teploty, vodík) a posílá varování. |
| `logger.rs` | Zapisovač na disk – ukládá měření do `data.csv` a chyby do `anomalies.log`. |
| `serial.rs` | Dělník pro USB – na pozadí čte sériový port a posílá balíčky do UI. |
| `demo.rs` | Simulátor – čte starší soubor `data.csv` a tváří se jako připojené auto. |
| `error.rs` | Seznam chyb – pojmenované chybové hlášky, aby program nikdy nespadl bez vysvětlení. |

---

## 3. Klíčové koncepty: Jak to funguje pod kapotou

### A. Dvě vlákna a kanál (mpsc) – Proč to nemáme v jednom vlákně?
- **Představ si problém:** Čtení z USB kabelu může trvat 20 milisekund nebo se na chvilku zaseknout. Kdybychom v tu chvíli zároveň kreslili okno, aplikace by zamrzala a myš by sekala.
- **Řešení:** 
  - Máme **Sériové vlákno** (`serial.rs`), které nedělá nic jiného, než že sedí u USB kabelu a sbírá bajty.
  - Máme **Grafické vlákno** (`main.rs` + `ui.rs`), které běží na 30 FPS a stará se o hezký obraz.
  - Mezi nimi je potrubí: `mpsc::channel()` (*Multi-Producer, Single-Consumer*). Sériové vlákno hodí vzorek do potrubí (`tx.send()`) a UI vlákno si ho každých 33 ms neblokujícím způsobem vytáhne (`rx.try_recv()`). Obraz je díky tomu dokonale plynulý.

### B. Proč posíláme binární data a ne text?
- Kdyby auto posílalo text: `"BATT:12.45V,CURR:3.21A\n"`, je to obrovské množství písmen, které mikrokontrolér musí převádět na text a počítač zase z textu zpátky na čísla. Žere to procesor a je to pomalé.
- My posíláme rovnou **surové bajty** přesně tak, jak leží v registrech čipu INA228 (28 bajtů).
- V `protocol.rs` vezmeme tyhle bajty a vynásobíme je tzv. **LSB konstantou** (nejmenší dílek senzoru, např. $0.0001953125\text{ V}$). Běží to bleskově.

### C. Co je to Znaménkové rozšíření (Sign extension)?
- Senzor INA228 měří proud jako 20bitové číslo se znaménkem. V počítači ale máme čísla buď 16bitová nebo 32bitová.
- Kdybychom 20bitové záporné číslo jen tak vrazili do 32bitové proměnné, počítač by si myslel, že je to obrovské kladné číslo (protože nejvyšší znaménkový bit by nebyl na 31. pozici).
- Funkce `sign_extend_20` udělá jednoduchý trik: posune bity doleva o 12 pozic a pak doprava o 12 pozic (`((val << 12) as i32) >> 12`). Tím se znaménko správně "rozlije" do celého 32bitového čísla.

### D. Kruhový buffer v DataManageru – Proč dávkové mazání?
- Graf ukazuje posledních 1200 bodů. Co se stane, když přijde bod číslo 1201?
- Špatné řešení: Smazat nultý prvek (`remove(0)`). To znamená, že počítač musí v paměti posunout všech zbývajících 1200 čísel o jedno doleva. A to při každém vzorku!
- **Naše chytré řešení (Batch Drain):** Necháme buffer narůst třeba na 1300 bodů a teprve potom jedním šmahem odřízneme prvních 100 starých bodů (`history.drain(0..100)`). Paměť se posouvá jen jednou za čas, což šetří spoustu výkonu.

### E. Ochrana před "špagetami v grafu" (Backward Time Jump)
- Když se auto restartuje, začne počítat čas zase od 0 ms. 
- Kdybychom to nechali být, graf by se pokusil spojit bod v čase 120 s s novým bodem v čase 0 s. Čára by se vrátila doleva přes celý displej ("Z-fold artefakt").
- V `datamanager.rs` hlídáme: *Je nový čas menší než předchozí?* Pokud ano, auto se restartovalo $\rightarrow$ vymažeme starý graf (`self.clear()`) a začínáme načisto.

### F. Žádné pády přes `.unwrap()` a vlastní chyby (`TelemetryError`)
- V začátečnickém kódu v Rustu lidi píšou všude `.unwrap()`. To znamená: "pokud je tu chyba, okamžitě program zabij (crashni)". Při závodě ale program NESMÍ spadnout jen proto, že někdo na sekundu vytáhl kabel nebo se nepovedlo otevřít soubor.
- V `error.rs` máme vlastní výčet `TelemetryError`. Když se něco pokazí, vlákno to zachytí, napíše do konzole srozumitelnou zprávu s názvem portu nebo souboru a aplikace běží dál.

### G. Konfigurace v `config.toml`
- Všechny limity (např. kdy hlásit přehřátí baterie nad 45 °C nebo kdy je proud moc velký) nejsou natvrdo zapsané v kódu.
- Jsou v souboru `config.toml`. Můžeš je otevřít v Poznámkovém bloku, přepsat číslo, spustit program a změna hned platí – nemusíš nic znovu programovat ani kompilovat.

### H. Čisté ukončení vláken (Graceful Shutdown) – `Arc<AtomicBool>`
- Když v aplikaci klikneš na **DISCONNECT** nebo zavřeš celé okno křížkem, nechceme, aby vlákna na pozadí dál běžela a žrala procesor.
- Používáme sdílenou atomickou vlajku `Arc<AtomicBool>`. Když aplikace končí, nastaví vlajku na `false`. Vlákno sériové linky i demo smyčka si toho v dalším cyklu všimnou a čistě se samy ukončí.

### I. Proč se CSV zapisuje jen v sériovém vlákně a ne v UI?
- Původně byl v kódu bug: data se zapisovala dvakrát – jednou v `serial.rs` a podruhé v `main.rs`.
- Navíc kdyby se zapisovalo v `main.rs`, Demo režim by simulovaná data zapisoval zpátky do souboru `data.csv`, ze kterého právě čte, a ten by nekonečně bobtnal.
- Zápis jsme nechali pouze v `serial.rs`. Grafické vlákno tak neztrácí čas zápisem na disk a ukládají se jen skutečná data z auta.

### J. Stavový automat spojení (`ConnectionState`)
- Místo obyčejného `true/false` (připojeno / odpojeno) máme výčet `ConnectionState`.
- Díky tomu aplikace ví, jestli je odpojená, připojená k reálnému autu (`Live`), v demo simulaci (`DemoMode`), nebo jestli nastala chyba.
- Navíc si pamatuje přesný čas připojení (`since: Instant`), takže v liště nahoře rovnou běží stopky, jak dlouho spojení trvá (např. `● LIVE [04:12]`).

### K. Klávesové zkratky a bezpečné psaní
- Během závodu v boxech je manipulace s myší nešikovná. Proto můžeš aplikaci řídit přímo z klávesnice:
  - `1`, `2`, `3`, `4`: Přepínání grafu mezi Napětím, Proudem, Výkonem a Energií.
  - `Mezerník`: Pozastavení a znovurozeběhnutí grafu (Pause / Resume).
  - `C`: Připojení k portu nebo odpojení (Connect / Disconnect).
  - `D`: Spuštění demo simulace (Demo).
- **Chytrý detail pro komisi**: Ošetřili jsme `!ctx.wants_keyboard_input()`. To znamená, že když píšeš kód jezdce (např. "SKL") nebo port do textového rámečku, písmena 'C' ani čísla '1-4' nespustí zkratky, ale normálně se píší do textu.

### L. Pauza v grafu (Tlačítko PAUSE / Mezerník)
- Když se na autě stane něco zajímavého (např. obrovský proudový výkyv při rozjezdu), telemetrický inženýr potřebuje graf zastavit, aby mu neujel z obrazovky.
- Po stisku mezerníku graf zamrzne, rozsvítí se zlaté varování `⏸ POZASTAVENO` a inženýr si může křivku v klidu prohlédnout. Po dalším stisku mezerníku se graf zase plynule rozjede.

### M. Automatická detekce portů (COM port dropdown)
- Už žádné ruční vypisování `"COM3"` nebo `"/dev/ttyUSB0"`.
- Funkce `serial::detect_available_ports()` se zeptá operačního systému na připojené převodníky a nabídne je v rozevíracím menu.
- Vedle je malé tlačítko `↻`, kterým můžeš porty kdykoliv znovu vyhledat, když za běhu zapojíš USB kabel.

### N. Měřič kvality spojení (PPS – Packets Per Second)
- Ukazuje, kolik datových balíčků za sekundu k nám z auta dorazí:
  - **Zelená (10 a víc PPS)**: Vynikající signál, data chodí plynule.
  - **Oranžová (1 až 9 PPS)**: Signál slábne nebo se data zpožďují.
  - **Červená (0 PPS)**: Výpadek spojení, z auta nic nechodí.

### O. Ukládání snímků obrazovky (Tlačítko SCREENSHOT)
- Jedním kliknutím na `SCREENSHOT` aplikace vyfotí celé své okno a pomocí knihovny `image` ho uloží do souboru jako `screenshot_<timestamp>.png`.
### P. Zákaz nebezpečného kódu (`unsafe_code = "forbid"`)
- V souboru `Cargo.toml` máme nastaveno `unsafe_code = "forbid"`.
- Znamená to, že kompilátor nám přísně zakazuje použít jakákoliv nebezpečná zadní vrátka jazyka C/C++ (jako surové ukazatele nebo obcházení typové kontroly).
- Celý náš program je 100% paměťově bezpečný – nehrozí v něm pád na neplatnou paměť ani přetečení vyrovnávací paměti (buffer overflow).

### Q. Profesionální vizitka projektu (`README.md`)
- Na GitHubu má projekt detailní návod a dokumentaci s architektonickým diagramem, popisem protokolů, tabulkou klávesových zkratek a seznamem použitých knihoven s odůvodněním, proč jsme je vybrali.

### R. Nový realistický závodní Demo režim (data.csv)
- Původní demo byla jen jednoduchá umělá sinusoida. Nyní jsme vytvořili realistický 80sekundový závodní okruh s opravdovou fyzikou:
  - **Fyzikální přesnost**: Výkon $P = V \times I$, úbytek napětí vlivem vnitřního odporu baterie ($V = V_{oc} - I \times R_{int}$), reálné zahřívání.
  - **Rekuperační brzdění**: Před zatáčkami auto prudce brzdí motorem $\rightarrow$ proud teče opačně do baterky ($I_{batt} = -1.25\,\text{A}$) a dobíjí ji!
  - **3 připravené reálné anomálie pro komisi**:
    1. **V čase ~10 s**: Na rovince dojde přetlak vodíku $\rightarrow$ napětí palivového článku spadne na 7.8 V (`FC V-SAG`). Následně odfoukne ventil a napětí se vrátí.
    2. **V čase ~22 s**: Při prudkém výjezdu ze zatáčky se zasekne servo řízení $\rightarrow$ proud baterie vystřelí na 16.8 A (`BATT OVERCURRENT`).
    3. **V čase ~50 s**: Po sérii rychlých zatáček se baterie zahřeje na 46.8 °C (`BATT OVERTEMP`).
  - Všechny tři anomálie okamžitě naskočí v bočním panelu i do `anomalies.log`.

### S. Interaktivní Zoom do grafu a tlačítko RECENTER
- **Proč to v telemetrii musí být:** Když se v autě objeví proudová špička nebo výpadek napětí, inženýr v boxech si potřebuje křivku přiblížit a prozkoumat přesné hodnoty.
- **Jak přiblížit bod v grafu:**
  - **Kolečkem myši**: Přibližuje přesně k bodu, na kterém stojíš myší!
  - **Pravým tlačítkem myši (Box Zoom)**: Natáhneš rámeček okolo zajímavého kousku grafu a ten se okamžitě zvětší na celou obrazovku.
  - **Tlačítka v liště**: `ZOOM IN (+)` (přiblíží 1.4×) a `ZOOM OUT (-)` (oddálí).
- **Chytrý detail pro komisi (žádné mizení bodů):**
  - Když je graf v živém režimu (`● LIVE`), ukazuje plovoucí okno (např. posledních 30 s).
  - Jakmile graf přiblížíš, přepne se do režimu `ZOOM`. V té chvíli aplikace začne do grafu posílat **všech 1200 bodů z historie**. Zkoumaný bod ti tedy neodjede ani nezmizí, i když v pozadí dál přicházejí nová data!
- **Jak se vrátit zpět na živý graf (`⟲ RECENTER`):**
  - Stačí kliknout na tlačítko `⟲ RECENTER` v liště grafu, zmáčknout klávesu **R**, nebo 2× poklepat kamkoliv do grafu.
  - Graf se odemkne, rozsvítí se zelené `● LIVE` a pokračuje v plynulém posouvání nových dat.
- **Odečet hodnot pod kurzorem:** V levém dolním rohu grafu svítí přesný čas a hodnota v bodě, kde máš kurzor (např. `t = 22.4 s | 16.85`).

### T. Barevné stupně anomálií (Zelená - Žlutá - Červená)
- **Proč to v závodě potřebujeme:** Když auto jede 4hodinový maraton, telemetrik nepotřebuje panikařit u každé drobné odchylky. Potřebuje na první pohled vědět, jestli je auto v pohodě, jestli má řidič zvolnit, nebo jestli hrozí okamžitý požár či zkrat:
  1. **Zelená (`● BEZ ANOMÁLIÍ`)**:
     - Všechny subsystémy (napětí, proud, teplota, ventilátory) pracují v normálních bezpečných mezích.
     - Nahoře v pravém panelu svítí zelený rámeček s nápisem *"Všechny subsystémy v normě"*.
  2. **Žlutá (`VAROVÁNÍ (MINOR)`)**:
     - Zvýšené namáhání: proud akumulátoru přesáhl 12 A, teplota článků vystoupala nad 40 °C nebo napětí palivového článku pokleslo pod 10.5 V.
     - Auto může bezpečně pokračovat, ale je to signál k úpravě chlazení nebo zvolnění tempa.
     - V historii anomálií svítí žlutě s označením `[WARN]`.
  3. **Červená (`KRITICKÉ RIZIKO`)**:
     - Bezprostřední nebezpečí: zkrat palivového článku (`FC SHORT`), proud akumulátoru nad 15 A (např. zaseknutý motor), přehřátí baterie nad 45 °C nebo kolaps napětí pod 9 V.
     - Riziko nevratného poškození LiPo baterie nebo difuzní membrány článku.
     - Stavový rámeček i záznamy v historii se rozzáří jasně červeně s označením `[CRIT]`.
- **Chytrý filtr proti zahlcení:** Když proud vyskočí na 16.8 A, systém nehlásí zbytečně žlutou i červenou chybu najednou. Díky kaskádovému vyhodnocení má červená kritická chyba přednost a žluté varování potlačí.

### U. Pokročilý pozávodní reporting a analytika (Klávesa P / Tlačítko REPORT)
- **Proč je to pecka pro závody i komisi:** Místo toho, aby program jen suše vyplivl pár čísel do souboru, stiskem klávesy **P** nebo kliknutím na **REPORT** vyskočí přímo v aplikaci moderní analytické okno s komplexním vyhodnocením celého stintu.
- **Klíčové metriky pro závodní strategii H2GP:**
  1. **Hybridní bilance (% FC)**: Ukazuje, kolik procent energie dodal vodíkový palivový článek a kolik LiPo baterka. Cíl v H2GP je $> 70\,\%$ z vodíku, aby se baterka nevybila dřív než za 4 hodiny vytrvalostního závodu.
  2. **Sklizeň rekuperace brzděním**: Kolik energie (J) a náboje (Ah) auto vrátilo zpět do baterky při brzdění před zatáčkami (kdy teče záporný proud $I_{batt} < 0$).
  3. **Extrémy výkonu a chlazení**: Špičkový výkon vč. minimálního napětí palivového článku (kontrola, jestli nehladověl na vodíku pod 9 V) a maximální teploty.
  4. **Záznam incidentů**: Chronologický přehled všech výstrah a kritických chyb s přesným časem.
- **Tři formy exportu pro tým:**
  - **HTML report (`reports/report_*.html`)**: Samostatná webová stránka s moderním tmavým designem, kterou si může kdokoliv v týmu otevřít v prohlížeči nebo vytisknout do PDF jako přílohu k maturitě.
  - **Markdown (`reports/report_*.md`)**: Tabulkový zápis se značkami vhodný pro dokumentaci a GitHub.
  - **Kopírování do schránky (Clipboard)**: Zkopíruje rychlý přehled do schránky pro vložení do chatu týmu.

---

## 4. Tahák na otázky k obhajobě (Otázky & Odpovědi)

**Otázka 1: Proč jste si pro telemetrii vybral právě jazyk Rust a ne Python nebo C#?**
> *Odpověď:* "Rust mi dává dvě obrovské výhody: extrémní rychlost a bezpečnost práce s pamětí. V závodní telemetrii potřebujeme jistotu, že aplikace nikdy nezamrzne kvůli Garbage Collectoru (jako v C# nebo Pythonu) a zároveň v ní nehrozí pády na neplatné ukazatele jako v C++. Navíc typový systém Rustu mi pomohl odchytit spoustu chyb už při kompilaci."

**Otázka 2: Co je to ten immediate-mode GUI framework egui?**
> *Odpověď:* "Běžná GUI (jako WPF nebo Qt) si drží stromy tlačítek v paměti a mění se pomocí událostí (Retained mode). `egui` funguje v Immediate mode – to znamená, že se celé rozhraní překresluje znovu 30krát za sekundu z aktuálních dat. Pro telemetrii je to ideální: nemusím složitě synchronizovat stav tlačítek s příchozími daty, prostě jen vezmu nejnovější hodnotu a vykreslím ji."

**Otázka 3: Jak řešíte vláknovou bezpečnost (Thread Safety) při zápisu do souboru?**
> *Odpověď:* "V souboru `logger.rs` používáme konstrukci `Arc<Mutex<BufWriter<File>>>`. `Arc` umožňuje sdílet přístup k souboru napříč vlákny, `Mutex` zaručuje, že v jeden okamžik zapisuje pouze jedno vlákno, a `BufWriter` ukládá data do vyrovnávací paměti v RAM, takže nezatěžujeme disk s každým jednotlivým řádkem."

**Otázka 4: Jak máte otestovanou správnost dekódování dat?**
> *Odpověď:* "V projektu máme sadu 68 automatizovaných unit testů (`cargo test`). Testujeme v nich hraniční hodnoty matematických posunů, třístupňovou detekci anomálií včetně souběžné detekce kanálových a auxiliárních chyb, výpočet hybridní bilance a rekuperace v modulu reportingu, HTML escapování, bezpečné prohledávání sériových portů, validaci konfigurace, automatické čištění bufferu při skocích v čase, logovací formátování a flush, funkce Zoomu a vycentrování grafu a ověřujeme, že se data z registrů INA228 přepočítávají s přesností na miliontiny voltu."

**Otázka 5: Co znamená zápis `unsafe_code = "forbid"` v Cargo.toml?**
> *Odpověď:* "To je striktní direktiva kompilátoru Rustu, která zakazuje použití jakéhokoliv 'unsafe' bloku v celém projektu. Dává nám to formální garanci, že aplikace je 100% bezpečná z hlediska práce s pamětí a nehrozí v ní přetečení vyrovnávací paměti ani nedefinované chování."

**Otázka 6: Jak funguje funkce Zoom a Recenter v grafu a co se děje s pamětí dat?**
> *Odpověď:* "Využíváme možnosti knihovny `egui_plot`. Když je graf v živém režimu, ořezáváme data na nastavené časové okno (např. 30 s), aby se graf automaticky posouval. Jakmile uživatel použije zoom (kolečko myši, obdélníkový výběr nebo tlačítka), graf přejde do režimu inspekce. V tom okamžiku začneme do vykreslování posílat celou kruhovou vyrovnávací paměť (až 1200 bodů), aby uživateli zkoumané body nemizely. Tlačítkem RECENTER, klávesou R nebo dvojklikem se pohled resetuje zpět do živého režimu."

**Otázka 7: Jak funguje rozlišení závažnosti anomálií a proč nehlásíte žlutou a červenou chybu najednou při vysokém proudu?**
> *Odpověď:* "Implementovali jsme třístupňový model: Zelená (`Nominal`), Žlutá (`Warning`) a Červená (`Critical`). Zelená svítí, když je vše v normě. Žlutá upozorňuje na zvýšené namáhání (např. proud baterie nad 12 A nebo teplota nad 40 °C). Červená hlásí bezprostřední nebezpečí zničení článku či zkrat (proud nad 15 A, teplota nad 45 °C, napětí pod 9 V nebo zkratový signál). Abychom operátora nezahltili duplicitními hláškami, vyhodnocení je kaskádové: při překročení kritické meze (např. 16.8 A) má přednost kritická červená anomálie a žluté varování se automaticky potlačí."

**Otázka 8: Jak ve vašem systému počítáte rekuperovanou energii a hybridní bilanci a jak funguje export do HTML?**
> *Odpověď:* "V modulu `report.rs` procházíme zaznamenanou historii vzorků. Kdykoliv má akumulátor záporný proud ($I_{batt} < -0.05\,\text{A}$), integrujeme výkon v čase ($E = V \cdot |I| \cdot \Delta t$), čímž získáme přesnou energii ušetřenou rekuperačním brzděním. Hybridní bilanci počítáme jako poměr energie dodané palivovým článkem vůči celkové spotřebě ($E_{fc} / E_{total} \times 100\,\%$). Pro export do HTML generujeme kompletní samostatnou stránku s vestavěnými CSS styly v čistém Rustu bez potřeby externích knihoven, což zajišťuje 100% spolehlivost i offline v depu."

---

## 5. Jak projekt spustit a předvést

1. **Spuštění aplikace v Demo režimu (bez auta):**
   ```bash
   cargo run
   ```
   - V aplikaci nahoře stačí stisknout klávesu **D** (nebo kliknout na **DEMO (D)**). Okamžitě začnou naskakovat živá data, kreslit se graf a nahoře se rozsvítí modré stopky `● DEMO [00:01]` a zelené počítadlo `10 PPS`.
   - V pravém bočním panelu ukaž komisi zelený stavový rámeček **`● BEZ ANOMÁLIÍ`** (*"Všechny subsystémy v normě"*).
   - Zmáčkni na klávesnici **1**, **2**, **3**, **4** – uvidíš, jak se okamžitě přepínají záložky grafu (Napětí, Proud, Výkon, Energie).
   - **Předveď Zoom a Recenter:**
     - Najeď myší na libovolný pík a zatoč kolečkem myši dopředu – graf se okamžitě přiblíží přesně k bodu pod myší.
     - V liště se rozsvítí zlatý nápis `ZOOM`.
     - V levém dolním rohu grafu ukaž komisi přesný odečet souřadnic (`t = ... s | ...`).
     - Vyzkoušej tlačítka `ZOOM IN (+)` a `ZOOM OUT (-)`.
     - Klikni na `⟲ RECENTER` nebo zmáčkni **R** (či 2× poklepej na graf) – graf se okamžitě vrátí zpět do živého toku a rozsvítí se zelené `● LIVE`.
   - **Předveď stupně anomálií a barevnou signalizaci:**
     - Během jízdy dema v časech anomálií sleduj pravý panel:
     - Při vzniku anomálie stavový odznak okamžitě přepne na **`KRITICKÉ RIZIKO`** a v historii naskakují barevné položky s přesným časem (`[CRIT]` červeně, `[WARN]` žlutě).
     - Jakmile anomálie pomine (ventil se odvzdušní, proud klesne), stavový odznak se v reálném čase sám vrátí na zelené **`● BEZ ANOMÁLIÍ`**!
   - **Předveď pozávodní analytický report (Klávesa P):**
     - Zmáčkni klávesu **P** nebo klikni na tlačítko `REPORT (P)` na horní liště.
     - Otevře se velké analytické okno: ukaž komisi **Podíl palivového článku**, **Sklizeň rekuperace**, špičkový výkon a seznam incidentů.
     - Klikni na **ULOŽIT HTML** – v liště se objeví potvrzovací hláška `Report uložen do reports/report_...html`.
     - Klikni na **KOPÍROVAT** – data se zkopírují do schránky.
     - Zmáčkni **Esc** pro zavření okna reportu.
   - Zmáčkni **Mezerník** – graf zamrzne v pauze a rozsvítí se žlutý nápis `⏸ POZASTAVENO`. Zmáčkni mezerník znovu a graf se zase rozjede.
   - Klikni na **SCREENSHOT** – vytvoří se PNG snímek obrazovky a v liště se objeví potvrzení o uložení.
   - Zmáčkni **C** nebo klikni na **DISCONNECT** pro čisté odpojení.

2. **Spuštění všech unit testů (pro komisi):**
   ```bash
   cargo test
   ```
   Ukáže 68 zelených testů (`68 passed; 0 failed; 1 ignored`).

3. **Zobrazení generované HTML dokumentace:**
   ```bash
   cargo doc --open --no-deps
   ```
   Otevře v prohlížeči kompletní interaktivní webovou dokumentaci projektu generovanou přímo ze zdrojových kódů.

