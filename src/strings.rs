//! All interface text, in English, Russian and Ukrainian. One row per string: `key: en, ru, uk;`.
//! Style: sentence case, no emoji, no trailing periods on labels, no exclamation marks.

use crate::i18n::Table;

macro_rules! strings {
    (
        text { $( $name:ident : $en:expr, $ru:expr, $uk:expr; )* }
    ) => {
        #[allow(dead_code)]
        pub struct S {
            $( pub $name: &'static str, )*
        }
        const EN: S = S { $( $name: $en, )* };
        const RU: S = S { $( $name: $ru, )* };
        const UK: S = S { $( $name: $uk, )* };
        pub static TABLE: Table<S> = Table { en: &EN, ru: &RU, uk: &UK };
    };
}

strings! {
    text {
        // Navigation
        nav_overview: "Overview", "Обзор", "Огляд";
        nav_sticks: "Sticks and drift", "Стики и дрейф", "Стіки та дрейф";
        nav_triggers: "Triggers", "Триггеры", "Тригери";
        nav_buttons: "Buttons and timing", "Кнопки и задержка", "Кнопки та затримка";
        nav_motion: "Touchpad and motion", "Тачпад и движение", "Тачпад і рух";
        nav_light: "Lightbar and rumble", "Подсветка и вибрация", "Підсвітка та вібрація";
        nav_settings: "Settings", "Настройки", "Налаштування";

        // Tray
        tray_open: "Open Padyx", "Открыть Padyx", "Відкрити Padyx";
        tray_exit: "Exit", "Выход", "Вихід";
        tray_no_pad: "No pad connected", "Геймпад не подключён", "Геймпад не підключено";

        // First run
        ob_title: "Test and set up your DualSense", "Проверьте и настройте свой DualSense", "Перевірте та налаштуйте свій DualSense";
        ob_lead: "Padyx shows every button, stick and trigger live, finds stick drift and sets the lightbar.", "Padyx показывает каждую кнопку, стик и триггер вживую, находит дрейф стиков и настраивает подсветку.", "Padyx показує кожну кнопку, стік і тригер наживо, знаходить дрейф стіків і налаштовує підсвітку.";
        ob_does: "What it does", "Что он делает", "Що він робить";
        ob_do_1: "Reads the pad over USB or Bluetooth and shows it live", "Читает геймпад по USB или Bluetooth и показывает его вживую", "Читає геймпад по USB або Bluetooth і показує його наживо";
        ob_do_2: "Tests sticks for drift and the shape of their travel", "Проверяет стики на дрейф и форму их хода", "Перевіряє стіки на дрейф і форму їхнього ходу";
        ob_do_3: "Sets the lightbar, player LEDs, rumble and trigger effects", "Настраивает подсветку, индикаторы игрока, вибрацию и эффекты триггеров", "Налаштовує підсвітку, індикатори гравця, вібрацію та ефекти тригерів";
        ob_local: "Everything stays on this computer. Padyx talks only to your pad.", "Всё остаётся на этом компьютере. Padyx общается только с вашим геймпадом.", "Усе залишається на цьому комп'ютері. Padyx спілкується лише з вашим геймпадом.";
        autostart: "Start with Windows", "Запускать вместе с Windows", "Запускати разом із Windows";
        autostart_os: "Start with the computer", "Запускать вместе с компьютером", "Запускати разом із комп'ютером";
        autostart_hint: "Applies your lightbar colour as soon as the pad connects, without opening the window", "Подсветка применяется сразу при подключении геймпада, окно не нужно", "Підсвітка застосовується одразу під час підключення геймпада, вікно не потрібне";
        ob_autostart_note: "You can change this in Settings at any time", "Это можно изменить в настройках в любой момент", "Це можна змінити в налаштуваннях будь-коли";
        ob_start: "Get started", "Начать", "Почати";

        // Status
        no_pad_title: "No pad connected", "Геймпад не подключён", "Геймпад не підключено";
        no_pad_body: "Connect a DualSense with a USB cable, or pair it over Bluetooth. It shows up here by itself.", "Подключите DualSense по кабелю USB или по Bluetooth. Он появится здесь сам.", "Підключіть DualSense кабелем USB або по Bluetooth. Він з'явиться тут сам.";
        bg_off_title: "The background part is not running", "Фоновая часть не запущена", "Фонову частину не запущено";
        bg_off_body: "It reads the pad and keeps the tray icon. Start it to see your pad.", "Она читает геймпад и держит значок в трее. Запустите её, чтобы увидеть геймпад.", "Вона читає геймпад і тримає значок у треї. Запустіть її, щоб побачити геймпад.";
        bg_start: "Start", "Запустить", "Запустити";
        conn_usb: "USB", "USB", "USB";
        conn_bt: "Bluetooth", "Bluetooth", "Bluetooth";
        conn_unknown: "Connecting", "Подключение", "Підключення";
        battery: "Battery", "Заряд", "Заряд";
        charging: "charging", "заряжается", "заряджається";
        full: "full", "полный", "повний";
        no_battery: "no data yet", "данных пока нет", "даних поки немає";
        polling: "Polling rate", "Частота опроса", "Частота опитування";
        pad_label: "Pad", "Геймпад", "Геймпад";
        demo_pad: "Demo pad", "Демо-геймпад", "Демо-геймпад";
        untested: "untested", "не проверен", "не перевірено";
        reduced_note: "The pad is in its reduced Bluetooth mode. Motion, touchpad and battery appear in a moment.", "Геймпад в урезанном режиме Bluetooth. Движение, тачпад и заряд появятся через мгновение.", "Геймпад в урізаному режимі Bluetooth. Рух, тачпад і заряд з'являться за мить.";
        ov_hint: "Press buttons and move the sticks: every control lights up", "Нажимайте кнопки и двигайте стики: каждый элемент подсвечивается", "Натискайте кнопки й рухайте стіки: кожен елемент підсвічується";
        trigger_l: "L2", "L2", "L2";
        trigger_r: "R2", "R2", "R2";

        // Sticks
        left_stick: "Left stick", "Левый стик", "Лівий стік";
        right_stick: "Right stick", "Правый стик", "Правий стік";
        stick_raw: "Raw", "Сырое", "Сире";
        stick_pos: "Position", "Положение", "Положення";
        stick_mag: "Distance", "Отклонение", "Відхилення";
        dz_hint: "The rings mark 5 % and 10 % of the travel", "Кольца отмечают 5 % и 10 % хода", "Кільця позначають 5 % і 10 % ходу";
        drift_title: "Drift test", "Тест дрейфа", "Тест дрейфу";
        drift_desc: "Put the pad down and do not touch the sticks for 5 seconds", "Положите геймпад и не трогайте стики 5 секунд", "Покладіть геймпад і не чіпайте стіки 5 секунд";
        drift_start: "Start drift test", "Начать тест дрейфа", "Почати тест дрейфу";
        drift_running: "Do not touch the sticks: {n} s", "Не трогайте стики: {n} с", "Не чіпайте стіки: {n} с";
        drift_ok: "No drift", "Дрейфа нет", "Дрейфу немає";
        drift_light: "Light drift", "Лёгкий дрейф", "Легкий дрейф";
        drift_bad: "Drift", "Дрейф", "Дрейф";
        drift_offset: "Resting offset", "Смещение в покое", "Зміщення в спокої";
        drift_jitter: "Jitter", "Дрожание", "Тремтіння";
        drift_rules: "OK: offset under 3 % and jitter under 1 %. Light drift: under 8 % and 3 %. Above that it is drift. Percent of the full travel.", "Норма: смещение меньше 3 % и дрожание меньше 1 %. Лёгкий дрейф: до 8 % и 3 %. Выше это дрейф. Проценты от полного хода.", "Норма: зміщення менше 3 % і тремтіння менше 1 %. Легкий дрейф: до 8 % і 3 %. Вище це дрейф. Відсотки від повного ходу.";
        circ_title: "Circularity test", "Тест круговой формы", "Тест колової форми";
        circ_desc: "Rotate the stick along the edge, a few full turns, then press Finish", "Прокрутите стик по краю, несколько полных кругов, затем нажмите «Готово»", "Прокрутіть стік по краю, кілька повних кіл, потім натисніть «Готово»";
        circ_start: "Start circularity test", "Начать тест формы", "Почати тест форми";
        circ_finish: "Finish", "Готово", "Готово";
        circ_coverage: "Turn covered", "Пройдено круга", "Пройдено кола";
        circ_error: "Average shape error", "Средняя ошибка формы", "Середня похибка форми";
        circ_reach: "Average reach", "Средний ход", "Середній хід";
        circ_incomplete: "The stick did not go all the way round, so the result is not reliable", "Стик не прошёл полный круг, результат ненадёжен", "Стік не пройшов повне коло, результат ненадійний";
        circ_rules: "A pad's gate is not a perfect circle. Under 6 % is normal, under 12 % acceptable, above that the shape is worn or damaged.", "Ограничитель стика не идеальный круг. Меньше 6 % нормально, меньше 12 % допустимо, выше форма изношена или повреждена.", "Обмежувач стіка не ідеальне коло. Менше 6 % нормально, менше 12 % допустимо, вище форма зношена або пошкоджена.";
        circ_ok: "Round", "Круглый", "Круглий";
        circ_light: "Slightly uneven", "Слегка неровный", "Трохи нерівний";
        circ_bad: "Uneven", "Неровный", "Нерівний";
        test_again: "Test again", "Повторить", "Повторити";
        cancel: "Cancel", "Отмена", "Скасувати";

        // Triggers
        trig_left: "Left trigger", "Левый триггер", "Лівий тригер";
        trig_right: "Right trigger", "Правый триггер", "Правий тригер";
        trig_value: "Value", "Значение", "Значення";
        trig_click: "Click", "Щелчок", "Клацання";
        trig_effects: "Adaptive trigger effects", "Адаптивные триггеры", "Адаптивні тригери";
        trig_effects_hint: "Pick an effect, then press the trigger. Effects switch off when you leave this page.", "Выберите эффект и нажмите триггер. Эффекты выключаются, когда вы уходите с этой страницы.", "Виберіть ефект і натисніть тригер. Ефекти вимикаються, коли ви залишаєте цю сторінку.";
        fx_off: "Off", "Выкл", "Вимк";
        fx_light: "Light resistance", "Лёгкое сопротивление", "Легкий опір";
        fx_heavy: "Heavy resistance", "Тяжёлое сопротивление", "Важкий опір";
        fx_weapon: "Weapon", "Оружие", "Зброя";
        fx_vibration: "Vibration", "Вибрация", "Вібрація";
        fx_unavailable: "Effects need a DualSense", "Для эффектов нужен DualSense", "Для ефектів потрібен DualSense";

        // Buttons and timing
        btn_checklist: "Press every control", "Нажмите каждый элемент", "Натисніть кожен елемент";
        btn_progress: "{n} of 17", "{n} из 17", "{n} із 17";
        btn_all_done: "Every control works", "Все элементы работают", "Усі елементи працюють";
        btn_reset: "Reset", "Сбросить", "Скинути";
        timing_title: "Report timing", "Время между отчётами", "Час між звітами";
        timing_rate: "Rate", "Частота", "Частота";
        timing_avg: "Average interval", "Средний интервал", "Середній інтервал";
        timing_max: "Longest interval", "Самый долгий интервал", "Найдовший інтервал";
        timing_jitter: "Jitter", "Разброс", "Розкид";
        timing_hint: "Each bar is the time between two reports from the pad, newest on the right", "Каждый столбец это время между двумя отчётами геймпада, новые справа", "Кожен стовпець це час між двома звітами геймпада, нові справа";
        b_up: "D-pad up", "Крестовина вверх", "Хрестовина вгору";
        b_down: "D-pad down", "Крестовина вниз", "Хрестовина вниз";
        b_left: "D-pad left", "Крестовина влево", "Хрестовина вліво";
        b_right: "D-pad right", "Крестовина вправо", "Хрестовина вправо";
        b_cross: "Cross", "Крест", "Хрест";
        b_circle: "Circle", "Круг", "Коло";
        b_square: "Square", "Квадрат", "Квадрат";
        b_triangle: "Triangle", "Треугольник", "Трикутник";
        b_l1: "L1", "L1", "L1";
        b_r1: "R1", "R1", "R1";
        b_l3: "L3 (stick click)", "L3 (нажатие стика)", "L3 (натискання стіка)";
        b_r3: "R3 (stick click)", "R3 (нажатие стика)", "R3 (натискання стіка)";
        b_create: "Create", "Create", "Create";
        b_options: "Options", "Options", "Options";
        b_ps: "PS button", "Кнопка PS", "Кнопка PS";
        b_touchpad: "Touchpad click", "Нажатие тачпада", "Натискання тачпада";
        b_mute: "Mute", "Mute", "Mute";

        // Touchpad and motion
        tp_title: "Touchpad", "Тачпад", "Тачпад";
        tp_finger: "Finger {n}", "Палец {n}", "Палець {n}";
        tp_none: "No finger", "Нет касания", "Немає дотику";
        tp_click: "Click", "Нажатие", "Натискання";
        gyro: "Gyroscope, degrees per second", "Гироскоп, градусов в секунду", "Гіроскоп, градусів за секунду";
        accel: "Accelerometer, g", "Акселерометр, g", "Акселерометр, g";
        axis_pitch: "Pitch", "Тангаж", "Тангаж";
        axis_yaw: "Yaw", "Рысканье", "Рискання";
        axis_roll: "Roll", "Крен", "Крен";
        orientation: "Orientation", "Положение", "Положення";
        orientation_hint: "Tilt follows the accelerometer", "Наклон берётся с акселерометра", "Нахил береться з акселерометра";
        no_motion: "Motion data is not available in this mode", "В этом режиме данных о движении нет", "У цьому режимі даних про рух немає";

        // Lightbar and rumble
        light_title: "Lightbar", "Подсветка", "Підсвітка";
        light_colour: "Colour", "Цвет", "Колір";
        light_custom: "Custom colour", "Свой цвет", "Свій колір";
        ch_red: "Red", "Красный", "Червоний";
        ch_green: "Green", "Зелёный", "Зелений";
        ch_blue: "Blue", "Синий", "Синій";
        light_brightness: "Brightness", "Яркость", "Яскравість";
        light_saved: "Saved and applied every time the pad connects", "Сохраняется и применяется при каждом подключении геймпада", "Зберігається й застосовується під час кожного підключення геймпада";
        players_title: "Player LEDs", "Индикаторы игрока", "Індикатори гравця";
        players_none: "None", "Нет", "Немає";
        players_n: "Player {n}", "Игрок {n}", "Гравець {n}";
        mute_led: "Mute button light", "Подсветка кнопки Mute", "Підсвітка кнопки Mute";
        rumble_title: "Rumble", "Вибрация", "Вібрація";
        rumble_left: "Left motor (strong, slow)", "Левый мотор (сильный, медленный)", "Лівий мотор (сильний, повільний)";
        rumble_right: "Right motor (light, fast)", "Правый мотор (слабый, быстрый)", "Правий мотор (слабкий, швидкий)";
        rumble_stop: "Stop", "Стоп", "Стоп";
        rumble_hint: "Rumble stops when you leave this page", "Вибрация выключается, когда вы уходите с этой страницы", "Вібрація вимикається, коли ви залишаєте цю сторінку";
        colour_violet: "Violet", "Фиолетовый", "Фіолетовий";

        // Settings
        settings_title: "Settings", "Настройки", "Налаштування";
        group_general: "General", "Основное", "Основне";
        autostart_failed: "Could not change the startup setting", "Не удалось изменить автозапуск", "Не вдалося змінити автозапуск";
        group_data: "Data", "Данные", "Дані";
        open_data_folder: "Open the data folder", "Открыть папку данных", "Відкрити теку даних";
        version: "Version", "Версия", "Версія";
        about: "About", "О программе", "Про програму";
        privacy_note: "Padyx reads your controller and talks only to it. Nothing leaves this computer except the update check and the links you open.", "Padyx читает ваш геймпад и общается только с ним. Ничего не уходит с компьютера, кроме проверки обновлений и ссылок, которые вы открываете.", "Padyx читає ваш геймпад і спілкується лише з ним. Нічого не залишає комп'ютер, крім перевірки оновлень і посилань, які ви відкриваєте.";

        // Linux access
        setup_title: "Allow Padyx to open the pad", "Разрешите Padyx открывать геймпад", "Дозвольте Padyx відкривати геймпад";
        setup_body: "Linux hides game controllers from programs until a udev rule says your user may use them. Padyx can add that rule for you. The system asks for your password and only this one file is installed.", "Linux не даёт программам доступ к геймпадам, пока правило udev не разрешит это вашему пользователю. Padyx может добавить такое правило. Система запросит пароль, устанавливается только этот один файл.", "Linux не дає програмам доступ до геймпадів, доки правило udev не дозволить це вашому користувачеві. Padyx може додати таке правило. Система запитає пароль, встановлюється лише цей один файл.";
        setup_install: "Install the rule", "Установить правило", "Встановити правило";
        setup_later: "Not now", "Не сейчас", "Не зараз";
        setup_done: "The rule is installed. Reconnect the pad if it does not appear.", "Правило установлено. Переподключите геймпад, если он не появился.", "Правило встановлено. Перепідключіть геймпад, якщо він не з'явився.";
        setup_failed: "The rule was not installed", "Правило не установлено", "Правило не встановлено";
        setup_file: "File", "Файл", "Файл";

        // Xbox emulation: navigation, tray, notices
        nav_emulation: "Xbox emulation", "Эмуляция Xbox", "Емуляція Xbox";
        tray_emulation: "Xbox emulation", "Эмуляция Xbox", "Емуляція Xbox";
        low_battery_title: "Controller battery is low", "Низкий заряд геймпада", "Низький заряд геймпада";
        low_battery_body: "The DualSense is at {n} %. Plug it in to charge.", "У DualSense осталось {n} %. Подключите его для зарядки.", "У DualSense залишилося {n} %. Підключіть його для зарядки.";

        // Xbox emulation: the page
        emu_intro: "Makes a DualSense look like an Xbox 360 controller, so every game that knows Xbox pads works with it.", "Делает DualSense похожим на геймпад Xbox 360, чтобы он работал в любой игре, которая знает геймпады Xbox.", "Робить DualSense схожим на геймпад Xbox 360, щоб він працював у будь-якій грі, яка знає геймпади Xbox.";
        emu_switch: "Xbox emulation", "Эмуляция Xbox", "Емуляція Xbox";
        emu_switch_hint: "A virtual Xbox 360 pad appears for each DualSense, and the real pad is hidden from games while it is on", "Для каждого DualSense появляется виртуальный геймпад Xbox 360, а настоящий геймпад скрыт от игр, пока эмуляция включена", "Для кожного DualSense з'являється віртуальний геймпад Xbox 360, а справжній геймпад прихований від ігор, поки емуляція ввімкнена";
        emu_simulated: "Simulated run: no driver is used and nothing is hidden", "Пробный запуск: драйверы не используются, ничего не скрывается", "Пробний запуск: драйвери не використовуються, нічого не приховується";
        emu_backend_missing: "The virtual pad driver is not installed", "Драйвер виртуального геймпада не установлен", "Драйвер віртуального геймпада не встановлено";
        emu_backend_noaccess: "Padyx has no access to the virtual pad driver", "У Padyx нет доступа к драйверу виртуального геймпада", "У Padyx немає доступу до драйвера віртуального геймпада";
        emu_backend_unsupported: "This system cannot make a virtual pad", "Эта система не умеет создавать виртуальный геймпад", "Ця система не вміє створювати віртуальний геймпад";
        emu_drivers: "Drivers", "Драйверы", "Драйвери";
        emu_drv_vigem_hint: "Makes the virtual Xbox 360 pad", "Создаёт виртуальный геймпад Xbox 360", "Створює віртуальний геймпад Xbox 360";
        emu_drv_hidhide_hint: "Hides the real DualSense from games, so a game never sees two pads", "Скрывает настоящий DualSense от игр, чтобы игра не видела два геймпада", "Приховує справжній DualSense від ігор, щоб гра не бачила два геймпади";
        emu_installed: "Installed", "Установлен", "Встановлено";
        emu_missing: "Not installed", "Не установлен", "Не встановлено";
        emu_install: "Install", "Установить", "Встановити";
        emu_official: "Official page", "Официальная страница", "Офіційна сторінка";
        emu_downloading: "Downloading {n} %", "Загрузка {n} %", "Завантаження {n} %";
        emu_verifying: "Checking the file", "Проверка файла", "Перевірка файлу";
        emu_launched: "The installer is open. Finish it, then come back here.", "Установщик открыт. Завершите установку и вернитесь сюда.", "Інсталятор відкрито. Завершіть встановлення й поверніться сюди.";
        emu_restart_note: "HidHide asks for a restart after it installs.", "После установки HidHide попросит перезагрузить компьютер.", "Після встановлення HidHide попросить перезавантажити комп'ютер.";
        emu_hash_failed: "The downloaded file does not match the expected checksum, so it was deleted and not run.", "Скачанный файл не совпал с ожидаемой контрольной суммой, поэтому он удалён и не запущен.", "Завантажений файл не збігся з очікуваною контрольною сумою, тому його видалено й не запущено.";
        emu_download_failed: "The download failed", "Не удалось скачать", "Не вдалося завантажити";
        emu_launch_failed: "The installer could not be started", "Не удалось запустить установщик", "Не вдалося запустити інсталятор";
        emu_driver_note: "Padyx does not ship these drivers. Install downloads the official release from GitHub, checks its checksum and starts it, and Windows asks for permission. You can also install them yourself from the official page.", "Padyx не поставляет эти драйверы. Кнопка загружает официальный релиз с GitHub, проверяет контрольную сумму и запускает его, Windows запросит разрешение. Драйверы можно поставить и самому с официальной страницы.", "Padyx не постачає ці драйвери. Кнопка завантажує офіційний реліз із GitHub, перевіряє контрольну суму й запускає його, Windows запитає дозвіл. Драйвери можна встановити й самому з офіційної сторінки.";
        emu_linux_title: "Virtual pad access", "Доступ к виртуальному геймпаду", "Доступ до віртуального геймпада";
        emu_uinput_ready: "uinput is ready", "uinput готов", "uinput готовий";
        emu_uinput_noaccess: "Padyx may not use /dev/uinput yet. Install the rule, then reconnect the pad.", "Padyx пока не может использовать /dev/uinput. Установите правило и переподключите геймпад.", "Padyx поки не може використовувати /dev/uinput. Встановіть правило й перепідключіть геймпад.";
        emu_uinput_missing: "The uinput kernel module is not loaded. Load it with: sudo modprobe uinput", "Модуль ядра uinput не загружен. Загрузите его командой: sudo modprobe uinput", "Модуль ядра uinput не завантажено. Завантажте його командою: sudo modprobe uinput";
        emu_pads: "Pads", "Геймпады", "Геймпади";
        emu_native: "Native mode (game supports DualSense)", "Родной режим (игра поддерживает DualSense)", "Рідний режим (гра підтримує DualSense)";
        emu_native_hint: "No virtual pad and no hiding: the game talks to the DualSense itself", "Без виртуального геймпада и без скрытия: игра работает с DualSense напрямую", "Без віртуального геймпада й без приховування: гра працює з DualSense напряму";
        emu_pad_emulated: "Emulated as Xbox 360", "Эмулируется как Xbox 360", "Емулюється як Xbox 360";
        emu_pad_native: "Native mode", "Родной режим", "Рідний режим";
        emu_pad_starting: "Starting", "Запуск", "Запуск";
        emu_pad_failed: "The virtual pad could not be made", "Виртуальный геймпад не создан", "Віртуальний геймпад не створено";
        emu_pad_idle: "Emulation is off", "Эмуляция выключена", "Емуляцію вимкнено";
        emu_hidden: "Real pad hidden from games", "Настоящий геймпад скрыт от игр", "Справжній геймпад прихований від ігор";
        emu_not_hidden: "Real pad is visible to games", "Настоящий геймпад виден играм", "Справжній геймпад видно іграм";
        emu_hide_failed: "Could not hide the real pad", "Не удалось скрыть настоящий геймпад", "Не вдалося приховати справжній геймпад";
        emu_restore: "Restore pad visibility", "Вернуть видимость геймпада", "Повернути видимість геймпада";
        emu_restore_hint: "Turns the emulation off and makes the real pad visible to every game again. Use it if a game cannot see your controller.", "Выключает эмуляцию и снова делает настоящий геймпад видимым для всех игр. Используйте, если игра не видит ваш геймпад.", "Вимикає емуляцію й знову робить справжній геймпад видимим для всіх ігор. Використовуйте, якщо гра не бачить ваш геймпад.";
        emu_restored: "The pad is visible again", "Геймпад снова виден", "Геймпад знову видно";
        emu_restore_failed: "Could not restore the pad", "Не удалось вернуть видимость", "Не вдалося повернути видимість";
        emu_zones: "Dead zones and response", "Мёртвые зоны и отклик", "Мертві зони та відгук";
        emu_zones_hint: "The dashed ring is the inner dead zone. The filled dot is your stick, the open ring is what the game gets.", "Пунктирное кольцо это внутренняя мёртвая зона. Закрашенная точка это ваш стик, пустое кольцо то, что получает игра.", "Пунктирне кільце це внутрішня мертва зона. Зафарбована точка це ваш стік, порожнє кільце те, що отримує гра.";
        emu_dz_inner: "Inner dead zone", "Внутренняя мёртвая зона", "Внутрішня мертва зона";
        emu_dz_outer: "Outer dead zone", "Внешняя мёртвая зона", "Зовнішня мертва зона";
        emu_dz_anti: "Anti dead zone", "Антимёртвая зона", "Антимертва зона";
        emu_curve: "Response curve", "Кривая отклика", "Крива відгуку";
        curve_linear: "Linear", "Линейная", "Лінійна";
        curve_smooth: "Smooth", "Плавная", "Плавна";
        curve_aggressive: "Aggressive", "Резкая", "Різка";
        curve_custom: "Custom", "Своя", "Своя";
        emu_curve_hint: "Smooth is finer near the centre, aggressive reacts sooner. Custom bends the line through two points.", "Плавная точнее у центра, резкая реагирует раньше. Своя изгибает линию по двум точкам.", "Плавна точніша біля центру, різка реагує раніше. Своя вигинає лінію за двома точками.";
        emu_point_n: "Point {n}", "Точка {n}", "Точка {n}";
        emu_point_in: "In", "Вход", "Вхід";
        emu_point_out: "Out", "Выход", "Вихід";
        emu_trig_zones: "Trigger dead zones", "Мёртвые зоны триггеров", "Мертві зони тригерів";
        emu_buttons: "Extra buttons", "Дополнительные кнопки", "Додаткові кнопки";
        emu_touchpad_as: "Touchpad click acts as", "Нажатие тачпада работает как", "Натискання тачпада працює як";
        emu_mute_as: "Mute acts as", "Mute работает как", "Mute працює як";
        emu_unmapped: "Nothing", "Ничего", "Нічого";
        emu_map_hint: "The other buttons follow their position: Cross is A, Circle is B, Square is X, Triangle is Y, Create is View, Options is Menu, PS is Guide.", "Остальные кнопки идут по положению: Крест это A, Круг это B, Квадрат это X, Треугольник это Y, Create это View, Options это Menu, PS это Guide.", "Решта кнопок іде за положенням: Хрест це A, Коло це B, Квадрат це X, Трикутник це Y, Create це View, Options це Menu, PS це Guide.";
        emu_rumble: "Rumble from games", "Вибрация из игр", "Вібрація з ігор";
        emu_rumble_intensity: "Intensity", "Сила", "Сила";
        emu_rumble_hint: "What a game sets as rumble is sent to the DualSense motors. 0 % turns it off.", "Вибрация, которую задаёт игра, передаётся на моторы DualSense. 0 % выключает её.", "Вібрація, яку задає гра, передається на мотори DualSense. 0 % вимикає її.";
        emu_before: "Raw", "Сырое", "Сире";
        emu_after: "To the game", "В игру", "У гру";
        xb_up: "D-pad up", "Крестовина вверх", "Хрестовина вгору";
        xb_down: "D-pad down", "Крестовина вниз", "Хрестовина вниз";
        xb_left: "D-pad left", "Крестовина влево", "Хрестовина вліво";
        xb_right: "D-pad right", "Крестовина вправо", "Хрестовина вправо";
        xb_start: "Menu (Start)", "Menu (Start)", "Menu (Start)";
        xb_back: "View (Back)", "View (Back)", "View (Back)";
        xb_l3: "Left stick click", "Нажатие левого стика", "Натискання лівого стіка";
        xb_r3: "Right stick click", "Нажатие правого стика", "Натискання правого стіка";
        xb_guide: "Guide", "Guide", "Guide";
    }
}
