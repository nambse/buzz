# Ortak özel kurulumunu çalıştırma

Bu komutlar yalnız bu Mac'teki mevcut `ortak-private-v0` kurulumunu yönetir.
Kalıcı dizin: `~/.local/share/ortak/private-v0`. Komutlar yeni çalışan, kimlik,
OAuth bağlantısı, container, image veya volume oluşturmaz.

```sh
~/.local/share/ortak/private-v0/bin/ortak open
~/.local/share/ortak/private-v0/bin/ortak status
~/.local/share/ortak/private-v0/bin/ortak start
~/.local/share/ortak/private-v0/bin/ortak stop
~/.local/share/ortak/private-v0/bin/ortak restart
```

`open` servisleri açıp doğrulanmış özel uygulamayı mevcut operatör kimliğiyle
başlatır. Repo veya geçici dizine ihtiyaç duymaz. Uygulama zaten bu araçla
açılmışsa ikinci süreç oluşturmaz; PID, başlangıç zamanı ve paket yolu eşleşir.
Başka konumdan açık eski özel paketi önce **Quit** ile kapatın. Güncel paket
kalıcı `desktop/b6373d98b1b99168f97317ee2fa7af1ccd4fc6176c109bda456b9585df10e7f8/Ortak Private.app`
altındadır. Kimlik ve OAuth değerleri komut satırına veya açılış günlüğüne yazılmaz.

Docker Desktop açık olmalıdır. `start` mevcut yedi container'ı ve dört kullanıcı
servisini açar. `stop` önce yeni istek girişini kapatır, en fazla45 saniye bekleyen
işlerin bitmesini bekler, sonra çalışan süreçleri ve veri servislerini kapatır.
Verileri silmez. İşler bitmezse worker ve veri servisleri açık kalır;
`lifecycle/operation.json` bunun kaydını tutar. `start` ile arayüz erişimini açıp
Activity'den işi inceleyebilir veya iptal edebilirsiniz. Daha sonra `stop` tekrar
denenebilir. Manuel kapanan servisler sonraki oturum açılışında da kapalı kalır;
`start` bu tercihi geri açar.

Yedinci container merkezi anlamsal yönlendirme servisidir; çalışan model
oturumlarından ayrıdır. Kapanışta OAuth yenileme sürecinin bitmesi için45 saniye
tanınır. Bu süreç hata veya zorla sonlandırmayla kapanırsa diğer depoları kapatma
başarı sayılmaz. `status` bu servisin de kimliğini ve erişim durumunu denetler;
sağlık okumak model çağrısı yapmaz. Gerçek yedi-container yeniden başlatma geçti.

`status` sabit container kimliği, image, disk bağlantıları, ağ adları ve yerel
portları kayıtla karşılaştırır. Native binary ve kurulu launcher dosyalarını
SHA256 ile doğrular. Servis işlemlerini, bekleyen kayıt sayılarını ve HTTP yanıt
durumlarını gösterir. HTTP401/403 kimlik doğrulamasının gerektiğini gösterir;
geçerli model bağlantısı veya çalışan sağlığı kanıtı sayılmaz. Status token
yüklemez ve gerçek model çağrısı yapmaz.

Native uygulama servislerden ayrı kalır. Kısa kesintide son başarılı Work
görünümünü koruyup yazmayı durdurur. Bağlantı geldikten sonra Employees **Refresh**,
Work **Refresh work**, Activity **Reload timeline** ile yeniden bağlanılabilir.
Güncel `330991b` paketi **Refresh work** eylemini Work bağlantıları ve bağımlılık
panellerine de bağlar. Aynı paket Deniz'in şifreli görüşmesini doğrulanmış
çalışan adıyla gösterir.
Employee dizini kesintisinde eski gateway `Offline`/`owner unavailable` etiketine
dönmez. Gerçek API kesintisi ve Refresh ile yeniden bağlantı bu pakette doğrulandı.

Kurulu launcher kendi Python dosyalarını kalıcı `lifecycle/code-…` dizininden
okur; çalışması repo dosyalarının yerinde olmasına bağlı değildir. Servis başına
`logs/{relay,api,worker,management}.log` en fazla4 MiB, üç döndürülmüş kopya tutar.
Önceki bootstrap günlükleri korunur. Çalışan servisler `dev.ortak.private-v0.*`
LaunchAgent'larıdır. Bu düzen, Docker Desktop'ın açıldığı bir kullanıcı oturumunu
gerektirir; FileVault kilidini açmaz veya Mac'i kendiliğinden başlatmaz.

İlk kayıt ve launcher kodu güncellemesi, yetkili checkout'tan seçili Python ile:

```sh
/Users/nambse/.pyenv/versions/3.12.8/bin/python3 scripts/ortak/private_stack.py install
/Users/nambse/.pyenv/versions/3.12.8/bin/python3 scripts/ortak/private_stack.py upgrade-launcher
~/.local/share/ortak/private-v0/bin/ortak restart
```

`install` mevcut kaydı tekrar kullanır. `upgrade-launcher` önce eski kaynak
sahipliğini doğrular, yalnız işletim kodunu dondurur ve önceki seçimi saklar.
Veritabanı şemasını, native binary veya Hermes image seçimini değiştirmez.
Bu komut genel ürün yükseltmesi veya yedekten geri yükleme aracı değildir.

Yetkili yeni bir özel uygulama derlemesini seçmek için önce açık paketten çıkın:

```sh
~/.local/share/ortak/private-v0/bin/ortak install-app --native-bundle '/tam/yol/Ortak Private.app'
~/.local/share/ortak/private-v0/bin/ortak open
```

`install-app` yalnız özel bundle kimliğini kabul eder; bütün dosyaları hash ile
doğrulanmış kalıcı kopyaya alır. Önceki paket ve uygulama verileri korunur.
Yarım kopya `.staging` olarak kalır ve kendiliğinden seçilmez; operatör incelemesi
gerektirir. Sonraki `open`, değişmiş paket veya bilinmeyen açık süreci reddeder.

Tutarlı yedek almak için önce Ortak Private uygulamasından **Quit** ile çıkın.
Ardından kurulu araç mevcut kaynakları kapatır, yedeği alır ve servisleri yeniden
açar. `--native-bundle` ile o anda kullanılan özel uygulama paketini seçin:

```sh
~/.local/share/ortak/private-v0/bin/ortak backup --native-bundle '/tam/yol/Ortak Private.app'
~/.local/share/ortak/private-v0/bin/ortak verify-backup --backup ~/.local/share/ortak/private-v0/backups/YEDEK_KIMLIGI
```

`backup` çıktısındaki `captured_not_restored`, henüz geri yükleme kanıtı değildir.
`verify-backup` yeni, ayrı ve ağsız hedeflerde gerçek dosya/DB geri yüklemesi yapar;
source volume'ları veya aktif yapılandırmayı değiştirmez. Doğrulanmış hedefler
korunur ve veritabanları doğrulama bitince kapatılır. Bu bir aktif kurulumun
yerine otomatik geçiş komutu değildir: klonlanmış worker/controller sahipliğini
yeniden bağlayıp devreye alma, doğrulanmış ayrı bir operatör adımı gerektirir.
Yedek özel kimlik/OAuth verilerini de içerir; tamamı yerel0700/0600 dosyalardadır.
Arşivleri veya içeriklerini Git'e eklemeyin.

Güncel schema80 yedeği `backups/31955fa962264f859d4e10e9ba6ffdec` altında alındı.
`recovery-verifications/436ff7a175494c64bb65888ee6efdfef/receipt.json` gerçek
ayrı hedef geri yükleme sonucudur (`verified_offline_stores`). Ana DB 156 ve
Honcho 26 tablonun bütün mantıksal satır hash'leri, katalog ve sequence değerleri
eşleşti. Beş soğuk volume'un dosya hash/mod/uid/gid/nanosaniye zamanları ve MinIO
metadata'sı eşleşti. Image arşiv denetimi ve gerçek Docker image load geçti.

Bu yedek Files girdisini ve mühürlü çalışma kopyasını, proje/konuşma/çalışan
belleği Stop kayıtlarını, şifreli DM'yi, anlamsal servis seçimini ve 330991b native
paketini içerir. Hermes journal integrity/foreign-key kontrolleri geçti:
28 kayıtlı run, bunların içinde 9 profile probe; ayrıca 2 şifreli run kaydı ve
1 workspace tool call korunuyor. Özel yapılandırma ve native dosyalar ayrı,
etkinleştirilmemiş dizinlere açılıp doğrulandı. SQLite WAL/SHM incelemesi kaynak
volume'a yazmadan sınırlı geçici kopyada yapılır. Doğrulama kaynak kurulumu
değiştirmedi; kaynak yedi container ve dört servisiyle yeniden açıldı.

Önceki yedek ve doğrulamalar da korunur. Yeni sonuç önceki altı-container
restart kabulünden ayrıdır; güncel yedi-container kapanış/açılışı yeni yedekte
uygulandı. Bu, tam Mac reboot veya klonlanmış runtime'ın etkinleştirilmesi
kanıtı değildir.

Kapatma ayrıca onaylı bellek yayın/geri çekme işlerini, çalışma alanı okuyucularını
ve şifreli mesaj işlemlerini denetler. Henüz denenmemiş gelecekteki süre sonu
geri çekmeleri saklanır; vadesi gelmiş veya belirsiz işler tamamlanmadan depolar
kapatılmaz. Bu ayrım gerçek PostgreSQL üzerinde iki üretim sorgusu testiyle doğrulandı.

Yeni gerçek kabulde `open` kalıcı paketi başlattı; ikinci çağrı aynı PID'yi döndürdü.
Native arayüz aynı operatör, önceki Office konuşmaları ve Deniz'in şifreli yanıtını
yeniden gösterdi. 29 yerel işletim testi geçti; iki ayrı PostgreSQL drain testi
disposable ortam seçilmediğinde bilinçli atlanır. Bu uygulama açılışı tam Mac
reboot kanıtının yerine geçmez.
