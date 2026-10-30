# Компактный интерфейс по ReplayLite

Запрос: компактное приложение с иконками с сайта, анимациями, ползунками и
подходом ReplayLite. База Lyra 47676cc, чистая ветка feat/desktop-app.
ReplayLite изучен на 6a7dc0c49361d758261fee87deca650c7c400da5: 760×480,
Lucide be09619c1c1124ad09c19fc3e87ee3b939409bbd, карточки/скругления,
шестерёнка/назад, отдельные настройки, 4 темы, RU/EN, отсутствие вечных анимаций.
C# код не переносится: используется общий подход, backend Lyra не меняется.

- [x] Главное окно 760×480; настройки отдельно, без постоянной боковой панели.
- [x] Встроенные Lucide SVG/лицензия, кнопки с tooltip и доступным именем.
- [x] Ползунки, toggle, темы mint/ocean/violet/light, RU/EN, отключаемая анимация.
- [x] Локальные настройки; не сохранять текст задачи/код/отчёт/логи автоматически.
- [x] Тесты прав/ошибок сохраняются; UI проверен в 760×480, минимум/масштаб,
      темы/EN/навигация/покой/отмена, Windows native CI/упаковка/хеши.
- [x] Документация, снимки, commit/push.

Новая optional dependency egui_extras/svg нужна для рендера встроенных SVG,
без web/HTTP loader. CLI dependency graph не расширяется. SVG хранятся локально,
цвет задаётся перед рендером; сеть при запуске не используется.
Сохранение настроек через AppData/Lyra (Linux XDG config), с ограничением размера,
валидацией и записью при изменении, а не каждый кадр. Анимации только событийные,
с конечной длительностью и отключением; spinner только во время операции.
Backend, произвольный shell, updater/трей/автозапуск ReplayLite не входят в UI-задачу.

## Локальные проверки перед CI

Полный fmt/Clippy/cargo test workspace с desktop проходит: 19 unit + 9 agent +
6 CLI + 3 desktop + 3 inference = 40; 2 helper и 2 real-GGUF smoke ignored
для отдельного явного запуска. Все прежние права task/apply/cancel сохранены.
UI headless перебирает 96 сочетаний размеров/тем/языков/страниц; проверяется
контраст ≥4.5, 18 встроенных SVG/PNG, пределы/неизвестные поля/перезапись preferences.
Настоящее окно Xvfb/Mesa --smoke-test (48 состояний) завершилось с кодом 0.
Лицензии Windows/Linux собраны, включая Lucide/Feather и LICENSES enum-map.
CLI без desktop не содержит GUI зависимостей. Скриншоты README — реальное
Linux-окно 760×480, без пользовательских данных. Windows/DPI/иконка PE проверяются CI.

Дополнен build-only winresource 0.1.34 (только Windows desktop) для иконки EXE;
конвертер tools/render_icon.py использует dev-only Rust example/Pillow.
В Windows ZIP Python не требуется. Готовые PNG/ICO закоммичены.
Версия alpha повышена до 0.2.0-dev.2.

## Проверки взаимодействий и найденные дефекты

В настоящем окне проверены четыре темы, RU→EN и сохранение light/en после
перезапуска, точный ввод CPU threads=3 в поле ползунка, выключение анимаций.
У custom toggle исправлены контраст knob (on_accent) и передача фокуса по клику:
Space теперь переключает после клика. Регрессия вводит реальные pointer/key events
в egui, проверяет press/release без двойного переключения.

Дополнительно воспроизведён выход JSON за 64 KiB из-за escape-символов при
допустимой длине строки. Сначала тест упал, затем save проверяет сериализованный
размер до записи. Предыдущий валидный файл сохраняется при отказе.
Полный локальный fmt/Clippy/40 тестов после исправлений проходит.

Native UI на Xvfb/Mesa: запуск с существующим Qwen2.5-Coder-0.5B GGUF, нажатие
Stop, исчезновение worker (остался только процесс окна), отсутствие temp-каталогов
и совпадение SHA исходника доверенной копии examples/buggy-add. Это проверка
отмены, не принятие качества кодинга 0.5B. Процессы наблюдались через ps;
первоначальная попытка обнаружить children через procfs не дала достоверного
наблюдения и не считается доказательством отсутствия worker.

Покой после работы/смен тем, debug Debian 13, Xvfb/Mesa: 2 s, 0 CPU ticks
(CLK_TCK=100), VmRSS/VmHWM 103140 KiB (~100.7 MiB), settings mtime неизменен.
Короткий локальный замер не переносится на Windows/7B/пользовательский ПК.
При WINIT_X11_SCALE_FACTOR=1.5 окно 760×480 заняло 1140×720 физических пикселей;
шрифты/иконки/компоновка визуально проверены. Это Linux DPI, не Windows 11 пользователя.

Первый CI eaf8c70 / 41056773336 — success Linux и Windows, в том числе ресурсы
EXE, SVG и native 48-state smoke. Финальный CI на исправления фокуса/контраста/
JSON-budget фиксируется после завершения ниже.

## Финальная принятая сборка

Source **1681ee92ca2e04f9a0b097b91b91bed71f6fb588**,
[CI 41059982110](https://github.com/DraimCiDo/Lyra/actions/runs/41059982110) —
**success Windows Server 2022 и Ubuntu 24.04**. fmt, Clippy, обычные тесты,
реальный CPU GGUF smoke, release, CLI smoke, native 48-state GUI и упаковка
прошли. Это не приёмка 7B/CUDA/Windows 11 пользователя.

[Windows ZIP](https://github.com/DraimCiDo/Lyra/actions/runs/41059982110/artifacts/12977834651):
11 024 608 байт, SHA-256
`bf3074cc29ce1f6d037edf30a204cbaf6a6551f174aab97f98bfdbb70afac8f8`.
Linux artifact 12978333119. Оба доступны до 2026-11-13.

Windows ZIP скачан и проверен: его SHA соответствует API digest; оба EXE
соответствуют SHA256SUMS.txt; source stamp, версия, VC Runtime prerequisite,
Lucide/Feather, egui_extras/resvg и font notices присутствуют; Markdown-ссылки
в архиве корректны; пользовательских настроек и весов нет.

| Бинарник | Размер | SHA-256 |
| --- | --- | --- |
| lyra-desktop.exe | 18 934 272 | 1fc682cc4118b36af905f6974942e6191a53d92dbd204dd4bd99107d2f62a199 |
| lyra.exe | 6 908 928 | 83b911691c27a0fb68b7901b925b971c1106121c0cd174738a713e9f32f64f8d |

PE: AMD64, desktop GUI subsystem 2, CLI CUI 3. В desktop проверены ресурсы
RT_ICON/RT_GROUP_ICON/RT_VERSION и 7 изображений ICO. Значок встроен в EXE,
а не только нарисован внутри окна. Дополнительный размер относительно GUI
прошлого этапа обусловлен SVG renderer и ресурсами; обещания ускорения модели нет.

Рабочая ветка feat/compact-desktop; task/apply/worker сохраняют прежние права.
Ручная целевая приёмка/ограничения: [DESKTOP.md](../DESKTOP.md).
