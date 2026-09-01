---
title: Объявление структур
sidebar:
  order: 4
---

## Макрос `#[amethystate]`

`#[amethystate]` заставляет обычную структуру Rust жить в store.

### Атрибуты структуры

```rust
#[amethystate(prefix = "network", version = 1, mode = "reactive", as_root)]
pub struct NetworkState { ... }
```

| Атрибут | Тип | Описание |
|-----------|------|-------------|
| `prefix` | `String` | Путь пространства имён в store. Обязателен для корневых структур. |
| `version` | `u32` | Версия схемы для миграций. По умолчанию `0`. |
| `mode` | `String` | Режим генерации кода: `"reactive"` (по умолчанию), `"persistent"` или `"both"`. |
|`as_root`| `flag` | С ним поля ложатся прямо в корень store, без пространства имён. |
| `on_unreadable` | вариант | Что делать со значением, которое не декодируется. `Refuse` (по умолчанию) или `UseDefault`. |
| `on_delete` | вариант | Что делает поле, когда под ним удалили его ключ. `Keep` (по умолчанию) или `UseDefault`. |
| `check` | `fn` | Правило про всю структуру целиком, идёт после того, как построены поля. |

Структура без `prefix` — вложенная: её встраивают в другую через `nested`.

`prefix` занимает место, которое называет, и всё под ним. Поэтому две структуры
над одним местом не уживутся — вторая при открытии получит отказ. Отсюда же
растёт то, как ведут себя `prefix` и `key` с точками:
[Кто владеет каким местом](/amethystate/ru/concepts/claims/).

### Атрибуты поля

Атрибуты поля необязательны. Поле без `#[amestate]` берёт значением
`Default::default()`, а ключом — своё имя.

```rust
#[amethystate(prefix = "app")]
pub struct AppState {
    pub counter: u32, // no annotation — uses Default::default(), stored as "app.counter"

    #[amestate(default = 8080)]
    pub port: u16,
}
```

| Атрибут | Тип | Описание |
|-----------|------|-------------|
| `default` | `Expr` | Значение на первом запуске. Без него — `Default::default()`. |
| `key` | `String` | Ключ, под которым поле лежит. По умолчанию — имя поля. |
| `nested` | флаг | Поле само — структура `#[amethystate]`. |
| `volatile` | флаг | Живёт только в памяти: store его не читает и не пишет, на каждом запуске оно равно изначально заданному. |
| `on_unreadable` | вариант | Что делать со значением, которое не декодируется, — вместо того, что сказала структура. |
| `on_delete` | вариант | Что делать, когда ключ поля удалили. |
| `check` | `fn` | Правило, которое обязано пройти каждое значение, приходящее из store. |

Список закрыт: макрос сверяет каждый атрибут и на незнакомом валит компиляцию,
перечислив те, что знает.

### Что происходит, когда значение испорчено

Три момента, у каждого свой ответ.

**Открытие.** Если по объявленному пути лежит то, что не декодируется в тип
поля, структура не построится и назовёт этот путь. Это `Refuse`, он по
умолчанию. `UseDefault` — для приложения, которое обязано запуститься при
любом раскладе: поле берёт значение из `default`, испорченное значение
остаётся на диске, чтобы его кто-нибудь починил, а
[`try_get`](/amethystate/ru/primitives/field/) отвечает `Err` — с самого
построения и до первого изменения, которое декодируется.

<!-- shown: a struct that opens over a value it cannot read -->
```rust
#[amethystate(prefix = "mixed", on_unreadable = UseDefault)]
pub struct Mixed {
    #[amestate(default = 8080u16)]
    pub port: u16,

    #[amestate(default = "".to_string(), on_unreadable = Refuse)]
    pub licence: String,
}
```
<!-- /shown -->

**Поле вправе ужесточить то, что сказала структура.** Выше настройки откроются
со сломанным `port`, а нечитаемый `licence` остановит всё. Обратное — `Refuse`
на структуре и `UseDefault` на поле — не соберётся, и компилятор назовёт это
поле. Вложенная структура наследует ответ той, что её держит, ужесточает его
так же и сверяется с ней на компиляции.

**Ключ удалили под живым полем.** Поле продолжает показывать последнее, что
держало, — то самое, что было на экране мгновение назад. Против него `default`
всего лишь догадка, сделанная на компиляции. `UseDefault` просит именно
догадку:

<!-- shown: a field that wants the default back when its key goes -->
```rust
#[amethystate(prefix = "mixed_delete")]
pub struct MixedDelete {
    #[amestate(default = 800u32)]
    pub width: u32,

    #[amestate(default = 600u32, on_delete = UseDefault)]
    pub height: u32,
}
```
<!-- /shown -->

**Пришло изменение, и оно не декодируется.** Поле держит последнее значение,
с которым store был согласен, подписчиков никто не зовёт. `try_get` про это
скажет и замолчит, как только очередное изменение декодируется. Объявлять тут
нечего.

### Значение декодируется, и оно бессмысленно

Всё выше — про байты, которые не читаются. Позиция окна −32000, кегль ноль и
имя темы, которую никто не ставил, декодируются прекрасно. `check` — то место,
где приложение говорит, что такого у него не будет.

Проверка поля — голая `fn`, берёт значение и контекст, а отвечает причиной, а
не `bool`. Причину покажет `try_get`, её же несёт отказ при открытии, — значит
и пишут её для того, кто потом будет чинить файл.

<!-- shown: a check on a field, and the world it is judged against -->
```rust
fn a_size_that_renders(size: &u8, _cx: &CheckContext) -> Result<(), Invalid> {
    if *size >= 6 {
        Ok(())
    } else {
        Err(Invalid::new("a font size below 6 renders nothing"))
    }
}

fn a_theme_that_is_installed(theme: &String, cx: &CheckContext) -> Result<(), Invalid> {
    let installed = cx.require::<InstalledThemes>()?;

    if installed.0.contains(&theme.as_str()) {
        Ok(())
    } else {
        Err(Invalid::new(format!("no theme called {theme} is installed")))
    }
}

#[amethystate(prefix = "checked_lenient", on_unreadable = UseDefault)]
pub struct LenientUi {
    #[amestate(default = 14u8, check = a_size_that_renders)]
    pub font_size: u8,

    #[amestate(default = "dark".to_string(), check = a_theme_that_is_installed)]
    pub theme: String,
}
```
<!-- /shown -->

Контекст нужен потому, что проверка — голая `fn`: она ничего не захватывает.
Мир, по которому она судит значение, — какие есть мониторы, какие темы
установлены, — передают store при открытии.

```rust
let store = StoreBuilder::new(settings)
    .context(InstalledThemes(installed))
    .build()?;
```

По одному значению на тип, спрашивают через `cx.get::<T>()` или
`cx.require::<T>()`. Если не передали ничего, `require` отклоняет значение:
проверка, которая не дотянулась до своего мира, не вправе назвать значение
хорошим.

Отклонённое значение — та же ситуация, что и у `on_unreadable`, и разбирают её
тем же способом. `Refuse` валит построение и называет путь и причину.
`UseDefault` берёт значение из `default`, оставляет сохранённое на диске и
отвечает на `try_get` через `Err`, пока не пройдёт какое-нибудь изменение.

Провалившееся построение отдаёт путь и причину фактами — типами, а не
предложением, — так что один отказ отличают от другого, не читая текст:

<!-- shown: telling one refused open from another -->
```rust
let refused = StrictUi::new_with(&store).unwrap_err();

let failed_at = facts::all::<Key, _>(&refused).next();
let said = facts::all::<Refused, _>(&refused).next();

match (refused.current_context(), failed_at, said) {
    (StorageError::Read, Some(Key(at)), Some(Refused(why))) => {
        eprintln!("{at} will not do: {why}")
    }
    _ => return Err(refused.into()),
}
```
<!-- /shown -->

При `UseDefault` ничего не падает, и те же два факта приходят через поле:

<!-- shown: asking a field what the store disagrees with -->
```rust
let held = match ui.font_size().try_get() {
    Ok(size) => size,
    Err(unread) => {
        let said = facts::all::<Refused, _>(&unread).next();

        match said {
            Some(Refused(why)) => eprintln!("running on the default: {why}"),
            None => eprintln!("the stored bytes will not decode"),
        }

        ui.font_size().get()
    }
};
```
<!-- /shown -->

`Refused` появляется только там, где значение завернула объявленная проверка.
Байты, которые не декодировались, несут `Key`, а под ним — фразу самого кодека.
Поле закрытого store отвечает `WriteError::Closed` — это третий, отдельный
случай: значение в нём последнее, что оно слышало. Что ещё можно спросить у
отчёта: [Ошибки](/amethystate/ru/concepts/errors/).

### Правило про структуру, а не про значение

Проверка поля видит одно значение. Соседей она не видит — поля строятся по
одному, и остальных ещё нет. Поэтому инвариант между двумя полями вешают на
структуру: её проверке отдают структуру целиком, когда все поля уже построены.

<!-- shown: a check on the struct, for what one field cannot see -->
```rust
fn the_window_can_be_drawn(
    window: &AmeData<LenientWindow>,
    _cx: &CheckContext,
) -> Result<(), Invalid> {
    if window.min <= window.max {
        Ok(())
    } else {
        Err(Invalid::new("the smallest window is wider than the largest")
            .at(&["min", "max"]))
    }
}

#[amethystate(
    prefix = "window_lenient",
    on_unreadable = UseDefault,
    check = the_window_can_be_drawn
)]
pub struct LenientWindow {
    #[amestate(default = 400u32)]
    pub min: u32,

    #[amestate(default = 1600u32)]
    pub max: u32,

    #[amestate(default = "amethystate".to_string())]
    pub title: String,
}
```
<!-- /shown -->

Приходит `AmeData<LenientWindow>` — близнец структуры с голыми полями, тот
самый, что получает и шаг миграции. Поэтому проверка читает `window.min`, а не
`window.min().get()`. Так и задумано: правило про отношение — это правило про
значения, а из чего сделана сама структура, решает её `mode`; близнец при любом
`mode` один и тот же.

`at` называет поля, о которых вердикт, и докладывают о нём только они: спросите
посторонний `title` — он ответит тем, что держит. Вердикт, не назвавший никого,
относится ко всем.

При `UseDefault` отклонённая структура **держит сохранённое**, а не сбрасывает
поля к их `default`. `default` объявляют значению, а отношению его объявить
негде, и в полях остаётся то, что говорит файл: жалоба придёт через `try_get`
на названных полях, а не тем, что значения поменяются под читателем.

Вложенную структуру улаживают раньше той, что её держит, поэтому проверка
родителя видит детей, чьи проверки уже прошли.

### Где проверка выполняется, а где нет

| значение приходит | проверка поля | проверка структуры |
| --- | --- | --- |
| структура строится | выполняется | выполняется |
| правка извне процесса | выполняется; отказ оставляет последнее хорошее значение и никого не будит | не выполняется |
| `load_with` | выполняется | выполняется |
| запись, сделанная самим этим процессом | не выполняется | не выполняется |
| шаг миграции | не выполняется | не выполняется |

Две строки отсюда стоит прочитать дважды.

**Ваш собственный `field.set(nonsense)` проверку не проходит**, а ложится на
диск; отказ придёт на следующем открытии. Значение, которое пишет сам процесс,
разбирают на входе — [перехватчиком](/amethystate/ru/concepts/subscriptions/):
он вправе отклонить запись до того, как та случится. Проверке отклонять уже
нечего, сохранённое сохранено.

**При `mode = "persistent"` нет `Field`, значит нет и `try_get`.** Отклонённое
значение при `UseDefault` берёт значение из `default` и пишет об этом в лог —
больше об этом нигде не скажут. Отклонённая *структура* при `UseDefault`
держит сохранённое, ровно как и везде, и тоже говорит об этом только в лог.
`Refuse`, который стоит по умолчанию, вместо этого валит загрузку; за ним и
тянитесь, когда загруженной структуре надо доверять.

<!-- shown: the same rule, on a struct that is loaded rather than watched -->
```rust
#[amethystate(prefix = "kept_window", mode = "persistent", check = the_kept_window_can_be_drawn)]
pub struct KeptWindow {
    #[amestate(default = 400u32)]
    pub min: u32,

    #[amestate(default = 1600u32)]
    pub max: u32,
}

fn the_kept_window_can_be_drawn(
    window: &AmeData<KeptWindow>,
    _cx: &CheckContext,
) -> Result<(), Invalid> {
    if window.min <= window.max {
        Ok(())
    } else {
        Err(Invalid::new("the smallest window is wider than the largest"))
    }
}
```
<!-- /shown -->

Три поля проверку не примут вовсе, и макрос скажет об этом на компиляции:

- **`volatile`-поле.** Из store в него ничего не приходит, значит проверку не о
  чем и спросить. Правило про значение, которое процесс пишет сам и никуда не
  кладёт, — забота
  [перехватчика](/amethystate/ru/concepts/subscriptions/).
- **`nested`-поле.** Проверка поля судит одно значение, пришедшее из store по
  одному пути. Вложенная структура — не значение, а поддерево путей: приходить
  ей неоткуда, и звать проверку не с чем. Правило, которое вы хотели, вешают на
  саму вложенную структуру — `#[amethystate(check = ..)]` над ней получает все
  её поля разом, когда она построена.
- **Карта.** Проверку объявляют один раз и про одно значение, а записи карты —
  данные: они приходят и уходят на ходу, и объявленного пути, на который
  повесить правило, у них нет. Что карта делает с записью, которую не может
  прочитать, — тема [Kv](/amethystate/ru/primitives/kv/), а не этой страницы.

## #[derive(AmeType)]

`#[derive(AmeType)]` позволяет положить обычную структуру Rust в поле
`#[amethystate]`. Он считает на компиляции `TYPE_HASH` по форме типа, и именно
это число проход миграции потом сличает, чтобы заметить: объявление изменилось
с тех пор, как записали данные.

Хеш — сводка, а не тождество. Две разные формы могут дать одно и то же число, и
тогда изменение проходит незамеченным, а про дрейф никто не скажет. Поднять
`version`, когда форма поменялась, — то, что от хеша не зависит.

```rust
#[derive(Debug, AmeType)]
pub struct CustomEndpoint {
    pub host: String,
    pub port: u16,
}
```

## Поля volatile

`volatile`-поле живёт только в памяти и на каждом запуске равно изначально
заданному. Годится для сиюминутного состояния интерфейса, которое хранить
незачем.

```rust
#[amethystate(prefix = "app")]
pub struct AppState {
    #[amestate(default = 8080)]
    pub port: u16,

    #[amestate(default = false, volatile)]
    pub loading: bool, // always starts as false, never written to disk
}
```

## Вложенные структуры

Структура без `prefix` — компонент: своего места в store у неё нет, её
встраивают в родительскую через `nested`. Префикс родителя встаёт впереди всех
вложенных полей.

```rust
#[amethystate]
pub struct DatabaseConfig {
    #[amestate(default = "localhost".to_string())]
    pub host: String,
}

#[amethystate(prefix = "sys")]
pub struct SystemSettings {
    #[amestate(nested)]
    pub db: DatabaseConfig, // stored as "sys.db.host"
}
```

## Как разделить одно место между двумя структурами

Одно место двум структурам не объявить — вторая при открытии получит отказ. Где
до одного значения надо дотянуться с двух сторон, адресуйте его по пути из той
структуры, которая его не объявляла: [Kv](/amethystate/ru/primitives/kv/)
читает и пишет всюду, где место не занято структурой. Где проходит эта граница,
решает [Кто владеет каким местом](/amethystate/ru/concepts/claims/).

## Хранение на уровне корня (`as_root`)

По умолчанию поля лежат под `prefix` структуры. С `as_root` они ложатся прямо в
корень store, без всякого пространства имён.

```rust
#[amethystate(mode = "persistent", as_root)]
pub struct AppConfig {
    #[amestate(default = "acme".to_string())]
    pub name: String,

    #[amestate(default = false)]
    pub verbose: bool,
}
```

Получается файл вида:

```toml
name = "acme"
verbose = false
```

Эта форма нужна, когда файл читает кто-то кроме этого крейта: конфиг, который
правят руками, или такой, чьи ключи другая программа уже ждёт на верхнем
уровне. Корневые поля занимают место так же, как любые другие, поэтому две
структуры, потянувшиеся к одному ключу, всё равно столкнутся:
[Кто владеет каким местом](/amethystate/ru/concepts/claims/).
