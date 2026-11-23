//! All user-facing demo text in the ten UI languages.
//!
//! `t(lang, key)` returns the label for a language code (en, cs, de, es, it, nb, pl, sv, ja,
//! zh); an unknown code falls back to English. Merchant and company names and the car's name
//! are not translated and live in the generators, not here.

macro_rules! texts {
    ($( $(#[$doc:meta])* $key:ident => [$en:expr, $cs:expr, $de:expr, $es:expr, $it:expr, $nb:expr, $pl:expr, $sv:expr, $ja:expr, $zh:expr]; )*) => {
        /// A translatable demo label.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum T {
            $( $(#[$doc])* $key, )*
        }

        /// Every key, in declaration order.
        pub const ALL: &[T] = &[$(T::$key,)*];

        /// Labels per key in the order of [`LANGUAGES`].
        const ROWS: &[[&str; 10]] = &[
            $( [$en, $cs, $de, $es, $it, $nb, $pl, $sv, $ja, $zh], )*
        ];
    };
}

/// Supported language codes; the column order of the text table.
pub const LANGUAGES: [&str; 10] = ["en", "cs", "de", "es", "it", "nb", "pl", "sv", "ja", "zh"];

/// The label for `key` in `lang`; unknown languages use English.
pub fn t(lang: &str, key: T) -> &'static str {
    let col = LANGUAGES.iter().position(|l| *l == lang).unwrap_or(0);
    ROWS.get(key as usize)
        .and_then(|row| row.get(col))
        .copied()
        .unwrap_or("")
}

texts! {
    /// Name of the current account.
    AccCurrent => ["Current account", "Běžný účet", "Girokonto", "Cuenta corriente", "Conto corrente", "Brukskonto", "Konto bieżące", "Lönekonto", "普通預金", "活期账户"];
    /// Name of the savings account.
    AccSavings => ["Savings account", "Spořicí účet", "Sparkonto", "Cuenta de ahorro", "Conto risparmio", "Sparekonto", "Konto oszczędnościowe", "Sparkonto", "貯蓄口座", "储蓄账户"];
    /// Name of the USD travel account.
    AccTravel => ["Travel account", "Cestovní účet", "Reisekonto", "Cuenta de viajes", "Conto viaggi", "Reisekonto", "Konto podróżne", "Resekonto", "旅行用口座", "旅行账户"];
    /// Salary payment and the planned cashflow item of the same name.
    Salary => ["Salary", "Výplata", "Gehalt", "Nómina", "Stipendio", "Lønn", "Wypłata", "Lön", "給与", "工资"];
    /// Yearly bonus paid in March.
    Bonus => ["Annual bonus", "Roční bonus", "Jahresbonus", "Bonus anual", "Bonus annuale", "Årlig bonus", "Premia roczna", "Årsbonus", "年間ボーナス", "年终奖金"];
    /// Tax refund received in May.
    TaxRefund => ["Tax refund", "Vratka daně", "Steuererstattung", "Devolución de impuestos", "Rimborso fiscale", "Skatterefusjon", "Zwrot podatku", "Skatteåterbäring", "税金の還付", "退税"];
    /// Monthly rent from the parking space.
    ParkingRent => ["Parking space rent", "Nájem parkovacího stání", "Miete Stellplatz", "Alquiler de la plaza de garaje", "Affitto posto auto", "Leie av parkeringsplass", "Czynsz za miejsce parkingowe", "Hyra parkeringsplats", "駐車場の賃料", "车位租金"];
    /// Interest paid on the savings account.
    SavingsInterest => ["Savings interest", "Úrok ze spořicího účtu", "Sparzinsen", "Intereses de ahorro", "Interessi sul risparmio", "Sparerente", "Odsetki od oszczędności", "Sparränta", "貯蓄利息", "储蓄利息"];
    /// Monthly mortgage instalment.
    MortgagePayment => ["Mortgage payment", "Splátka hypotéky", "Hypothekenrate", "Cuota de la hipoteca", "Rata del mutuo", "Terminbeløp boliglån", "Rata kredytu hipotecznego", "Bolåneavbetalning", "住宅ローン返済", "房贷还款"];
    /// The one extra mortgage repayment.
    ExtraMortgage => ["Extra mortgage repayment", "Mimořádná splátka hypotéky", "Sondertilgung Hypothek", "Amortización extra de la hipoteca", "Rimborso anticipato del mutuo", "Ekstra avdrag på boliglånet", "Nadpłata kredytu hipotecznego", "Extra amortering på bolånet", "住宅ローンの繰上返済", "房贷提前还款"];
    /// Monthly kitchen loan instalment.
    KitchenLoanPayment => ["Kitchen loan payment", "Splátka úvěru na kuchyň", "Rate Küchenkredit", "Cuota del préstamo de la cocina", "Rata del prestito per la cucina", "Termin kjøkkenlån", "Rata kredytu na kuchnię", "Avbetalning kökslån", "キッチンローン返済", "厨房贷款还款"];
    /// Monthly building fee payment and the property's recurring cost of the same name.
    BuildingFee => ["Building fee", "Příspěvek na správu domu", "Hausgeld", "Cuota de comunidad", "Spese condominiali", "Felleskostnader", "Czynsz administracyjny", "Avgift till bostadsrättsföreningen", "管理費", "物业费"];
    /// Electricity bill.
    Electricity => ["Electricity", "Elektřina", "Strom", "Electricidad", "Elettricità", "Strøm", "Prąd", "El", "電気代", "电费"];
    /// Heating bill.
    Heating => ["Heating", "Vytápění", "Heizung", "Calefacción", "Riscaldamento", "Oppvarming", "Ogrzewanie", "Uppvärmning", "暖房費", "取暖费"];
    /// Internet bill.
    Internet => ["Internet", "Internet", "Internet", "Internet", "Internet", "Internett", "Internet", "Internet", "インターネット", "宽带网络"];
    /// Mobile phone plan.
    MobilePlan => ["Mobile plan", "Mobilní tarif", "Handyvertrag", "Plan móvil", "Piano telefonico", "Mobilabonnement", "Abonament komórkowy", "Mobilabonnemang", "携帯電話料金", "手机套餐"];
    /// Video streaming subscription.
    Streaming => ["Streaming subscription", "Předplatné streamovací služby", "Streaming-Abo", "Suscripción de streaming", "Abbonamento streaming", "Strømmeabonnement", "Subskrypcja serwisu streamingowego", "Streamingabonnemang", "動画配信サービス", "视频流媒体订阅"];
    /// Music streaming subscription.
    MusicSub => ["Music subscription", "Předplatné hudební služby", "Musik-Abo", "Suscripción de música", "Abbonamento musicale", "Musikkabonnement", "Subskrypcja muzyczna", "Musikabonnemang", "音楽配信サービス", "音乐订阅"];
    /// Cloud storage subscription.
    CloudStorage => ["Cloud storage", "Cloudové úložiště", "Cloud-Speicher", "Almacenamiento en la nube", "Spazio cloud", "Skylagring", "Miejsce w chmurze", "Molnlagring", "クラウドストレージ", "云存储"];
    /// Gym membership fee.
    Gym => ["Gym membership", "Členství v posilovně", "Fitnessstudio", "Cuota del gimnasio", "Abbonamento palestra", "Treningssenter", "Karnet na siłownię", "Gymkort", "ジムの会費", "健身房会费"];
    /// Public transport pass.
    TransitPass => ["Transit pass", "Předplatní jízdenka MHD", "ÖPNV-Ticket", "Abono de transporte", "Abbonamento trasporti", "Månedskort kollektivtransport", "Bilet miesięczny komunikacji miejskiej", "Månadskort kollektivtrafik", "定期券", "公交月票"];
    /// Life insurance premium payment.
    PremLife => ["Life insurance premium", "Pojistné za životní pojištění", "Beitrag Lebensversicherung", "Prima del seguro de vida", "Premio assicurazione vita", "Premie livsforsikring", "Składka ubezpieczenia na życie", "Livförsäkringspremie", "生命保険料", "人寿保险保费"];
    /// Home insurance premium payment.
    PremHome => ["Home insurance premium", "Pojistné za pojištění domácnosti", "Beitrag Hausratversicherung", "Prima del seguro del hogar", "Premio assicurazione casa", "Premie hjemforsikring", "Składka ubezpieczenia mieszkania", "Hemförsäkringspremie", "住宅保険料", "房屋保险保费"];
    /// Car insurance premium payment.
    PremCar => ["Car insurance premium", "Pojistné za pojištění auta", "Beitrag Kfz-Versicherung", "Prima del seguro del coche", "Premio assicurazione auto", "Premie bilforsikring", "Składka ubezpieczenia samochodu", "Bilförsäkringspremie", "自動車保険料", "车险保费"];
    /// Liability insurance premium payment.
    PremLiability => ["Liability insurance premium", "Pojistné za pojištění odpovědnosti", "Beitrag Haftpflichtversicherung", "Prima del seguro de responsabilidad civil", "Premio assicurazione responsabilità civile", "Premie ansvarsforsikring", "Składka ubezpieczenia OC", "Ansvarsförsäkringspremie", "個人賠償責任保険料", "责任险保费"];
    /// Travel insurance premium payment.
    PremTravel => ["Travel insurance premium", "Pojistné za cestovní pojištění", "Beitrag Reiseversicherung", "Prima del seguro de viaje", "Premio assicurazione viaggio", "Premie reiseforsikring", "Składka ubezpieczenia podróżnego", "Reseförsäkringspremie", "旅行保険料", "旅行保险保费"];
    /// Yearly property tax payment and the property's recurring cost of the same name.
    PropertyTax => ["Property tax", "Daň z nemovitosti", "Grundsteuer", "Impuesto sobre bienes inmuebles", "Imposta sugli immobili", "Eiendomsskatt", "Podatek od nieruchomości", "Fastighetsavgift", "固定資産税", "房产税"];
    /// Groceries payments and the planned cashflow item of the same name.
    Groceries => ["Groceries", "Nákup potravin", "Lebensmittel", "Compra de alimentos", "Spesa alimentare", "Dagligvarer", "Zakupy spożywcze", "Matinköp", "食料品", "食品杂货"];
    /// Drugstore purchases.
    Drugstore => ["Drugstore", "Drogerie", "Drogerie", "Droguería", "Prodotti per la casa e la persona", "Hygieneartikler", "Drogeria", "Hygienartiklar", "ドラッグストア", "日用品"];
    /// Restaurant meals.
    Restaurant => ["Restaurant", "Restaurace", "Restaurant", "Restaurante", "Ristorante", "Restaurant", "Restauracja", "Restaurang", "外食", "餐厅"];
    /// Coffee shop visits.
    Cafe => ["Café", "Kavárna", "Café", "Cafetería", "Caffè", "Kafé", "Kawiarnia", "Fik", "カフェ", "咖啡馆"];
    /// Delivered meals.
    FoodDelivery => ["Food delivery", "Rozvoz jídla", "Essenslieferung", "Comida a domicilio", "Cibo a domicilio", "Matlevering", "Dostawa jedzenia", "Hemleverans av mat", "フードデリバリー", "外卖"];
    /// Fuel for the car.
    Fuel => ["Fuel", "Palivo", "Tanken", "Gasolina", "Carburante", "Drivstoff", "Paliwo", "Bensin", "ガソリン代", "加油"];
    /// Car maintenance and repairs.
    CarService => ["Car service", "Servis auta", "Autowerkstatt", "Taller del coche", "Tagliando auto", "Bilverksted", "Serwis samochodu", "Bilverkstad", "車の整備", "汽车保养"];
    /// Parking fees.
    Parking => ["Parking", "Parkování", "Parken", "Aparcamiento", "Parcheggio", "Parkering", "Parkowanie", "Parkering", "駐車料金", "停车费"];
    /// Online shop order.
    OnlineOrder => ["Online order", "Online objednávka", "Online-Bestellung", "Pedido online", "Ordine online", "Nettbestilling", "Zamówienie online", "Onlinebeställning", "ネット注文", "网购"];
    /// Furniture purchase.
    Furniture => ["Furniture", "Nábytek", "Möbel", "Muebles", "Mobili", "Møbler", "Meble", "Möbler", "家具", "家具"];
    /// Clothes shopping.
    Clothes => ["Clothes", "Oblečení", "Kleidung", "Ropa", "Abbigliamento", "Klær", "Ubrania", "Kläder", "衣料品", "服装"];
    /// Electronics purchase.
    Electronics => ["Electronics", "Elektronika", "Elektronik", "Electrónica", "Elettronica", "Elektronikk", "Elektronika", "Elektronik", "家電・電子機器", "电子产品"];
    /// Sports equipment.
    SportsGear => ["Sports gear", "Sportovní vybavení", "Sportausrüstung", "Equipamiento deportivo", "Attrezzatura sportiva", "Sportsutstyr", "Sprzęt sportowy", "Sportutrustning", "スポーツ用品", "运动装备"];
    /// Pharmacy purchase.
    Pharmacy => ["Pharmacy", "Lékárna", "Apotheke", "Farmacia", "Farmacia", "Apotek", "Apteka", "Apotek", "薬局", "药店"];
    /// Dentist visit.
    Dentist => ["Dentist", "Zubař", "Zahnarzt", "Dentista", "Dentista", "Tannlege", "Dentysta", "Tandläkare", "歯科", "牙医"];
    /// Cinema tickets.
    Cinema => ["Cinema", "Kino", "Kino", "Cine", "Cinema", "Kino", "Kino", "Bio", "映画館", "电影院"];
    /// Concert tickets.
    ConcertTickets => ["Concert tickets", "Vstupenky na koncert", "Konzertkarten", "Entradas de concierto", "Biglietti per un concerto", "Konsertbilletter", "Bilety na koncert", "Konsertbiljetter", "コンサートチケット", "演唱会门票"];
    /// Video games.
    Games => ["Games", "Hry", "Spiele", "Videojuegos", "Videogiochi", "Spill", "Gry", "Spel", "ゲーム", "游戏"];
    /// Books.
    Books => ["Books", "Knihy", "Bücher", "Libros", "Libri", "Bøker", "Książki", "Böcker", "書籍", "书籍"];
    /// Taxi ride.
    Taxi => ["Taxi ride", "Jízda taxíkem", "Taxifahrt", "Viaje en taxi", "Corsa in taxi", "Taxitur", "Przejazd taksówką", "Taxiresa", "タクシー代", "打车"];
    /// Flight tickets for a trip.
    Flights => ["Flight tickets", "Letenky", "Flugtickets", "Billetes de avión", "Biglietti aerei", "Flybilletter", "Bilety lotnicze", "Flygbiljetter", "航空券", "机票"];
    /// Lodging on a trip.
    Accommodation => ["Accommodation", "Ubytování", "Unterkunft", "Alojamiento", "Alloggio", "Overnatting", "Nocleg", "Boende", "宿泊費", "住宿"];
    /// Card spending during the long trip (USD account).
    HolidaySpending => ["Holiday spending", "Výdaje na dovolené", "Urlaubsausgaben", "Gastos de vacaciones", "Spese di vacanza", "Utgifter på ferie", "Wydatki wakacyjne", "Semesterutgifter", "旅行中の支出", "度假消费"];
    /// December gifts.
    Gifts => ["Gifts", "Dárky", "Geschenke", "Regalos", "Regali", "Gaver", "Prezenty", "Presenter", "プレゼント", "礼物"];
    /// Standing order to the savings account.
    TransferSavings => ["Transfer to savings", "Převod na spoření", "Überweisung aufs Sparkonto", "Transferencia al ahorro", "Trasferimento al risparmio", "Overføring til sparing", "Przelew na oszczędności", "Överföring till sparande", "貯蓄口座へ振替", "转入储蓄"];
    /// Monthly deposit to the broker.
    BrokerDeposit => ["Broker deposit", "Vklad k brokerovi", "Einzahlung beim Broker", "Ingreso en el bróker", "Versamento al broker", "Innskudd hos megler", "Wpłata na rachunek maklerski", "Insättning till depån", "証券口座への入金", "转入证券账户"];
    /// Money sent for a crypto purchase.
    CryptoPurchase => ["Crypto purchase", "Nákup kryptoměn", "Krypto-Kauf", "Compra de criptomonedas", "Acquisto di criptovalute", "Kjøp av krypto", "Zakup kryptowalut", "Kryptoköp", "暗号資産の購入", "购买加密货币"];
    /// Top-up of the USD travel account.
    TravelTopUp => ["Travel account top-up", "Dobití cestovního účtu", "Aufladung Reisekonto", "Recarga de la cuenta de viajes", "Ricarica del conto viaggi", "Påfyll reisekonto", "Zasilenie konta podróżnego", "Insättning på resekontot", "旅行用口座へ入金", "旅行账户充值"];
    /// Transfer out of the savings account (top-up of the current account).
    TransferFromSavings => ["Transfer from savings", "Převod ze spoření", "Überweisung vom Sparkonto", "Transferencia desde el ahorro", "Trasferimento dal risparmio", "Overføring fra sparing", "Przelew z oszczędności", "Överföring från sparande", "貯蓄口座から振替", "从储蓄转出"];
    /// Savings withdrawal for the car.
    CarPurchase => ["Car purchase", "Koupě auta", "Autokauf", "Compra del coche", "Acquisto dell'auto", "Bilkjøp", "Zakup samochodu", "Bilköp", "車の購入", "购买汽车"];
    /// Savings withdrawal for the parking space.
    ParkingPurchase => ["Parking space purchase", "Koupě parkovacího stání", "Kauf Stellplatz", "Compra de la plaza de garaje", "Acquisto del posto auto", "Kjøp av parkeringsplass", "Zakup miejsca parkingowego", "Köp av parkeringsplats", "駐車場の購入", "购买车位"];
    /// Savings withdrawal for the gold coins.
    GoldPurchase => ["Gold coins purchase", "Nákup zlatých mincí", "Kauf von Goldmünzen", "Compra de monedas de oro", "Acquisto di monete d'oro", "Kjøp av gullmynter", "Zakup złotych monet", "Köp av guldmynt", "金貨の購入", "购买金币"];
    /// Proceeds of a partial crypto sale paid to the current account.
    CryptoSale => ["Crypto sale", "Prodej kryptoměn", "Krypto-Verkauf", "Venta de criptomonedas", "Vendita di criptovalute", "Salg av krypto", "Sprzedaż kryptowalut", "Kryptoförsäljning", "暗号資産の売却", "出售加密货币"];
    /// Museum tickets on a trip.
    MuseumTickets => ["Museum tickets", "Vstupenky do muzea", "Museumstickets", "Entradas de museo", "Biglietti del museo", "Museumsbilletter", "Bilety do muzeum", "Museibiljetter", "美術館のチケット", "博物馆门票"];
    /// Appliances for the new kitchen.
    KitchenAppliances => ["Kitchen appliances", "Kuchyňské spotřebiče", "Küchengeräte", "Electrodomésticos de cocina", "Elettrodomestici da cucina", "Kjøkkenapparater", "Sprzęt AGD do kuchni", "Köksapparater", "キッチン家電", "厨房电器"];
    /// Name of the flat (owner-occupied property).
    PropFlat => ["Flat", "Byt", "Wohnung", "Piso", "Appartamento", "Leilighet", "Mieszkanie", "Lägenhet", "マンション", "公寓"];
    /// Name of the parking space (investment property).
    PropParking => ["Parking space", "Parkovací stání", "Stellplatz", "Plaza de garaje", "Posto auto", "Parkeringsplass", "Miejsce parkingowe", "Parkeringsplats", "駐車場", "车位"];
    /// Note on the flat.
    FlatNote => ["Owner-occupied, bought with a mortgage", "Vlastní bydlení, koupeno na hypotéku", "Eigennutzung, mit Hypothek finanziert", "Vivienda propia, comprada con hipoteca", "Prima casa, acquistata con un mutuo", "Egen bolig, kjøpt med boliglån", "Mieszkanie własne, kupione na kredyt hipoteczny", "Eget boende, köpt med bolån", "自宅、住宅ローンで購入", "自住，贷款购买"];
    /// Note on the parking space.
    ParkingNote => ["Rented out to a neighbour", "Pronajato sousedovi", "An einen Nachbarn vermietet", "Alquilada a un vecino", "Affittato a un vicino", "Utleid til en nabo", "Wynajęte sąsiadowi", "Uthyrd till en granne", "近所の方に賃貸中", "租给了邻居"];
    /// One-time property cost: the kitchen.
    CostKitchen => ["New kitchen", "Nová kuchyně", "Neue Küche", "Cocina nueva", "Cucina nuova", "Nytt kjøkken", "Nowa kuchnia", "Nytt kök", "新しいキッチン", "新厨房"];
    /// Note on each property valuation.
    ValuationNote => ["Estimate", "Odhad", "Schätzung", "Estimación", "Stima", "Estimat", "Szacunek", "Uppskattning", "推定額", "估价"];
    /// Name of the mortgage.
    LoanMortgage => ["Mortgage", "Hypotéka", "Hypothek", "Hipoteca", "Mutuo", "Boliglån", "Kredyt hipoteczny", "Bolån", "住宅ローン", "房贷"];
    /// Name of the kitchen loan.
    LoanKitchen => ["Kitchen loan", "Úvěr na kuchyň", "Küchenkredit", "Préstamo de la cocina", "Prestito per la cucina", "Kjøkkenlån", "Kredyt na kuchnię", "Kökslån", "キッチンローン", "厨房贷款"];
    /// Home insurance policy name.
    PolHome => ["Home insurance", "Pojištění domácnosti", "Hausratversicherung", "Seguro del hogar", "Assicurazione casa", "Hjemforsikring", "Ubezpieczenie mieszkania", "Hemförsäkring", "住宅保険", "房屋保险"];
    /// Life insurance policy name.
    PolLife => ["Life insurance", "Životní pojištění", "Lebensversicherung", "Seguro de vida", "Assicurazione sulla vita", "Livsforsikring", "Ubezpieczenie na życie", "Livförsäkring", "生命保険", "人寿保险"];
    /// Car insurance policy name.
    PolCar => ["Car insurance", "Pojištění auta", "Kfz-Versicherung", "Seguro del coche", "Assicurazione auto", "Bilforsikring", "Ubezpieczenie samochodu", "Bilförsäkring", "自動車保険", "车险"];
    /// Liability insurance policy name.
    PolLiability => ["Liability insurance", "Pojištění odpovědnosti", "Haftpflichtversicherung", "Seguro de responsabilidad civil", "Assicurazione di responsabilità civile", "Ansvarsforsikring", "Ubezpieczenie OC", "Ansvarsförsäkring", "個人賠償責任保険", "责任险"];
    /// Travel insurance policy name.
    PolTravel => ["Travel insurance", "Cestovní pojištění", "Reiseversicherung", "Seguro de viaje", "Assicurazione di viaggio", "Reiseforsikring", "Ubezpieczenie podróżne", "Reseförsäkring", "旅行保険", "旅行保险"];
    /// Home policy limit: contents.
    LimContents => ["Household contents", "Vybavení domácnosti", "Hausrat", "Contenido del hogar", "Contenuto dell'abitazione", "Innbo", "Wyposażenie mieszkania", "Lösöre", "家財", "家庭财产"];
    /// Home policy limit: water damage.
    LimWater => ["Water damage", "Škody vodou", "Leitungswasserschäden", "Daños por agua", "Danni da acqua", "Vannskade", "Szkody wodne", "Vattenskador", "水濡れ損害", "水损"];
    /// Life policy limit: death benefit.
    LimDeath => ["Death benefit", "Pojistné plnění při smrti", "Todesfallleistung", "Capital por fallecimiento", "Capitale in caso di morte", "Dødsfallsdekning", "Świadczenie na wypadek śmierci", "Dödsfallsbelopp", "死亡保険金", "身故保险金"];
    /// Car policy limit: own damage.
    LimOwnDamage => ["Own damage", "Havarijní pojištění", "Kaskoschaden", "Daños propios", "Danni al proprio veicolo", "Skade på egen bil", "Szkody własne", "Skada på egen bil", "車両損害", "车辆损失"];
    /// Liability limit title (car and liability policies).
    LimLiability => ["Liability limit", "Limit odpovědnosti", "Haftungssumme", "Límite de responsabilidad", "Massimale di responsabilità", "Ansvarsgrense", "Limit odpowiedzialności", "Ansvarsgräns", "賠償限度額", "赔偿限额"];
    /// Travel policy limit: medical expenses.
    LimMedical => ["Medical expenses", "Léčebné výlohy", "Behandlungskosten", "Gastos médicos", "Spese mediche", "Medisinske utgifter", "Koszty leczenia", "Sjukvårdskostnader", "治療費", "医疗费用"];
    /// One-ounce gold coin (the car's name is not translated and has no key).
    AssetGold => ["Gold coin, 1 oz", "Zlatá mince, 1 oz", "Goldmünze, 1 Unze", "Moneda de oro, 1 oz", "Moneta d'oro, 1 oncia", "Gullmynt, 1 unse", "Złota moneta, 1 uncja", "Guldmynt, 1 uns", "金貨 1オンス", "金币 1 盎司"];
    /// Name of the euro government bond.
    BondName => ["Euro government bond 2.9 %", "Eurový státní dluhopis 2,9 %", "Euro-Staatsanleihe 2,9 %", "Bono del Estado en euros 2,9 %", "Titolo di Stato in euro 2,9 %", "Statsobligasjon i euro 2,9 %", "Euroobligacja skarbowa 2,9 %", "Statsobligation i euro 2,9 %", "ユーロ建て国債 2.9%", "欧元国债 2.9%"];
    /// Spending plan: holiday.
    PlanHoliday => ["Holiday", "Dovolená", "Urlaub", "Vacaciones", "Vacanze", "Ferie", "Wakacje", "Semester", "休暇", "度假"];
    /// Spending plan: Christmas and gifts.
    PlanChristmas => ["Christmas and gifts", "Vánoce a dárky", "Weihnachten und Geschenke", "Navidad y regalos", "Natale e regali", "Jul og gaver", "Święta i prezenty", "Jul och julklappar", "クリスマスとプレゼント", "圣诞节与礼物"];
    /// Spending plan: car.
    PlanCar => ["Car", "Auto", "Auto", "Coche", "Auto", "Bil", "Samochód", "Bil", "車", "汽车"];
    /// Tag group for stock positions.
    TagGroupStrategy => ["Strategy", "Strategie", "Strategie", "Estrategia", "Strategia", "Strategi", "Strategia", "Strategi", "戦略", "策略"];
    /// Tag: core holdings.
    TagCore => ["Core", "Jádro", "Kern", "Núcleo", "Nucleo", "Kjerne", "Trzon", "Kärna", "コア", "核心"];
    /// Tag: growth holdings.
    TagGrowth => ["Growth", "Růst", "Wachstum", "Crecimiento", "Crescita", "Vekst", "Wzrost", "Tillväxt", "成長", "成长"];
    /// Tag: dividend holdings.
    TagDividend => ["Dividend", "Dividendy", "Dividende", "Dividendos", "Dividendi", "Utbytte", "Dywidenda", "Utdelning", "配当", "股息"];
    /// Planned cashflow item for subscriptions.
    CfSubscriptions => ["Subscriptions", "Předplatná", "Abos", "Suscripciones", "Abbonamenti", "Abonnementer", "Subskrypcje", "Prenumerationer", "サブスクリプション", "订阅"];
    /// Attached document name on the flat.
    DocPurchaseContract => ["Purchase contract", "Kupní smlouva", "Kaufvertrag", "Contrato de compraventa", "Contratto di acquisto", "Kjøpekontrakt", "Umowa kupna", "Köpekontrakt", "売買契約書", "购房合同"];
    /// Attached document name on a policy.
    DocPolicy => ["Policy document", "Pojistná smlouva", "Versicherungspolice", "Póliza de seguro", "Polizza assicurativa", "Forsikringsbevis", "Polisa ubezpieczeniowa", "Försäkringsbrev", "保険証券", "保险单"];
    /// Short note on a watchlist stock.
    WatchNote1 => ["Waiting for a better entry price", "Čekám na lepší nákupní cenu", "Warte auf einen besseren Einstiegskurs", "Esperando un mejor precio de entrada", "Aspetto un prezzo d'ingresso migliore", "Venter på en bedre inngangspris", "Czekam na lepszą cenę wejścia", "Väntar på ett bättre ingångspris", "より良い買い場を待つ", "等待更好的买入价"];
    /// Short note on a watchlist stock.
    WatchNote2 => ["Interesting long-term, review every quarter", "Zajímavé z dlouhodobého hlediska, kontrola každé čtvrtletí", "Langfristig interessant, quartalsweise prüfen", "Interesante a largo plazo, revisar cada trimestre", "Interessante nel lungo periodo, da rivedere ogni trimestre", "Interessant på lang sikt, gjennomgås hvert kvartal", "Ciekawa na lata, przegląd co kwartał", "Intressant på lång sikt, ses över varje kvartal", "長期で注目、四半期ごとに見直す", "长期看好，每季度复查"];
    /// Trip payment: a meal or a coffee on holiday (category travel).
    HolidayMeal => ["Meal on holiday", "Jídlo na dovolené", "Essen im Urlaub", "Comida en vacaciones", "Pasto in vacanza", "Måltid på ferie", "Posiłek na wakacjach", "Måltid på semestern", "旅行中の食事", "度假餐饮"];
    /// Trip payment: local transport on holiday (category travel).
    HolidayTransport => ["Transport on holiday", "Doprava na dovolené", "Fahrten im Urlaub", "Transporte en vacaciones", "Trasporti in vacanza", "Transport på ferie", "Transport na wakacjach", "Resor på semestern", "旅行中の交通費", "度假交通"];
    /// Trip payment: shopping on holiday (category travel).
    HolidayShopping => ["Shopping on holiday", "Nákupy na dovolené", "Einkäufe im Urlaub", "Compras en vacaciones", "Acquisti in vacanza", "Innkjøp på ferie", "Zakupy na wakacjach", "Inköp på semestern", "旅行中の買い物", "度假购物"];
    /// Counterparty of the tax refund.
    TaxOffice => ["Tax office", "Finanční úřad", "Finanzamt", "Agencia Tributaria", "Agenzia delle Entrate", "Skatteetaten", "Urząd Skarbowy", "Skatteverket", "税務署", "税务局"];
    /// Counterparty of the property tax.
    Municipality => ["City council", "Městský úřad", "Stadtverwaltung", "Ayuntamiento", "Comune", "Kommunen", "Urząd Miasta", "Kommunen", "市役所", "市政府"];
    /// Counterparty of the building fee.
    OwnersAssociation => ["Owners' association", "Společenství vlastníků", "Eigentümergemeinschaft", "Comunidad de propietarios", "Condominio", "Sameiet", "Wspólnota mieszkaniowa", "Bostadsrättsföreningen", "管理組合", "业主委员会"];
    /// Counterparty of the transit pass.
    PublicTransport => ["Public transport", "Dopravní podnik", "Verkehrsbetriebe", "Transporte público", "Trasporto pubblico", "Kollektivtransport", "Komunikacja miejska", "Kollektivtrafiken", "交通局", "公交公司"];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_is_filled_in_every_language() {
        assert!(ALL.len() >= 80);
        for lang in LANGUAGES {
            for key in ALL {
                let s = t(lang, *key);
                assert!(!s.trim().is_empty(), "{lang} {key:?} is empty");
                assert!(
                    !s.ends_with('.') && !s.ends_with('。'),
                    "{lang} {key:?}: {s}"
                );
            }
        }
    }

    #[test]
    fn no_two_languages_share_the_whole_table() {
        for (i, a) in LANGUAGES.iter().enumerate() {
            for b in &LANGUAGES[i + 1..] {
                let same = ALL.iter().filter(|k| t(a, **k) == t(b, **k)).count();
                assert!(same * 2 < ALL.len(), "{a} and {b} share {same} labels");
            }
        }
    }

    #[test]
    fn unknown_language_falls_back_to_english() {
        for key in ALL {
            assert_eq!(t("xx", *key), t("en", *key));
        }
        assert_eq!(t("cs", T::AccCurrent), "Běžný účet");
    }
}
