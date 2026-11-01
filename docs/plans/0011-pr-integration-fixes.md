# Согласование PR #2/#3 и исправления обзора

Статус: реализация готова; фактические проверки — ниже.
Исходные ревизии: #3 caeed74, #2 ce2406a.

## Результат и границы

Объединить Context Optimizer с Desktop LocalChat/CloudChat/Task/Project,
сохранить роли сообщений при передаче в GGUF, ограничить общее время облачного
task, восстановить CUDA CI и привести документацию к фактическому состоянию.
Новые подсистемы roadmap, публикация релиза и вызовы платных моделей не входят.

## План и критерии

- [x] Проверить актуальность веток и воспроизвести конфликт chat.rs.
- [x] Общий выбор истории: Safe/all, Balanced/12, Aggressive/6 для всех payload.
- [x] Передавать структурные сообщения в llama.cpp chat template; тест без GPU.
- [x] Общий deadline cloud task; HTTP timeout ограничен оставшимся временем.
- [x] Установить CUDA Visual Studio integration; проверить preflight через CI.
- [x] Исправить устаревшие README/HTTP_GATEWAY/PARITY и описание PR.
- [x] Проверить fmt, Clippy, тесты, сборки и smoke через Windows/Linux CI.

## Проверки

Локально нет Cargo/rustc/MSVC в PATH; системные настройки не меняются.
Для форматирования используется отдельный rustfmt 1.99.1 в work/, проверенный
по SHA256 из официального manifest Rust. Компиляция и тесты — native CI.
До изменений Windows CI caeed74 успешен, Linux остановился до проверок,
CUDA CMake сообщил No CUDA toolset found. Annotation Linux job 124859707011
подтвердила No space left on device при восстановлении старого whole-target cache.
Реальный GPU runtime не проверен.

Код исправлений: cd96b7d1572cdfaae2d587d70c765c515f09f778.
В [Windows/Linux CI](https://github.com/DraimCiDo/Lyra/actions/runs/41314354327)
обе платформы прошли fmt, Clippy и tests с desktop, затем Clippy/tests с
desktop,gateway-http, а также реальный CPU GGUF smoke, release build,
проверки стартовых проектов, запуск Desktop и упаковку артефактов.
[CUDA CI](https://github.com/DraimCiDo/Lyra/actions/runs/41314354223) прошёл установку
и проверку Visual Studio integration; результат отдельной компиляции/упаковки
доступен по этой ссылке. Этот runner не проверяет запуск на NVIDIA GPU.
Локально rustfmt --check и git diff --check завершились с кодом 0;
116 относительных Markdown-ссылок существуют. PR #2 является предком ветки
(merge-base --is-ancestor: 0) и закрыт как включённый в #3.

## Передача состояния

Работа выполняется в ветке #3. Включение ветки #2 сохраняет её историю;
main не изменяется. Результаты сборок привязаны к cd96b7d; следующие изменения
README/Desktop/Project Map/Roadmap и этого отчёта затрагивают только Markdown.
Окончательный итог проверки CUDA также фиксируется в описании PR #3.
