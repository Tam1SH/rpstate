---
title: Кто владеет каким местом
sidebar:
  order: 16
---

`prefix` структуры говорит, где она сидит, а поля её — пути под этим префиксом.
Написать одно и то же место двум структурам ничто не мешает: ключ с точками
достаёт так же глубоко, как и префикс с точками.

<!-- shown: two structs that want the same place -->
```rust
#[amethystate(prefix = "ui", version = 1)]
pub struct Ui {
    #[amestate(key = "panels.left.visible", default = true)]
    pub left_panel_visible: bool,
}

#[amethystate(prefix = "ui.panels", version = 1)]
pub struct Panels {
    #[amestate(key = "left.visible", default = true)]
    pub left_visible: bool,
}
```
<!-- /shown -->

Сложите префикс с ключом, и обе упрутся в один путь: `ui.panels.left.visible`.

## Вторая получает отказ

Store держит таблицу занятого, и вторая структура, открывающаяся над тем же
местом, не построится:

<!-- shown: what the refusal looks like -->
```rust
let _ui = Ui::new_with(&store)?;

let refused = Panels::new_with(&store)
    .expect_err("`ui.panels.left.visible` is spelled by both of them");

assert_eq!(refused.current_context(), &StorageError::Claimed);

for claim in all::<Claimed, _>(&refused) {
    println!("{} claims {}", claim.by, claim.path);
}
```
<!-- /shown -->

Ошибка — `StorageError::Claimed`, и отчёт несёт по факту `Claimed` на каждую
сторону: путь и схему, которой он понадобился. Достают их из отчёта по типу, а
не поиском по напечатанному тексту.

Отказ — дешёвый конец того же столкновения. Две структуры, пишущие в один путь,
— это одна структура, молча затирающая значение другой на каждом сохранении, а
чтобы найти это потом, надо прочесть два объявления, которые друг друга не
поминают. Отказ же приходит на том вызове, который открывает структуру, и
называет обе.

## Что считается пересечением

Два места пересекаются, когда одно держит другое. В этом всё правило, и оно
симметрично: неважно, какое объявлено первым, важно только, что одно поддерево
начинает другое.

| два места | пересекаются |
| --- | --- |
| `ui` и `ui.panels` | да — второе внутри первого |
| `ui.panels` и `ui` | да — та же пара, только наоборот |
| `ui.panels` и `ui.status` | нет |
| `ui` и `ui!x` | нет — `ui!x` другое имя, просто начинается с тех же букв |

На поле чужой структуры префикс тоже не ляжет. Поле — такое же место, как всякое
другое, поэтому структура с префиксом `root.b` получит отказ, если поле `b` под
`root` уже объявила другая.

Места, которые не встречаются, никто не трогает, как бы близко они ни сидели:

<!-- shown: a struct that sits right beside one and still opens -->
```rust
#[amethystate(prefix = "ui.panels.right", version = 1)]
pub struct RightPanel {
    #[amestate(key = "visible", default = true)]
    pub visible: bool,
}
```
<!-- /shown -->

Ближе уже некуда: `Ui` держит `ui.panels.left.visible`, `RightPanel` —
`ui.panels.right.visible`. Расходятся они на последнем общем уровне, ни один
путь не начинает другой, и обе открываются в одном store. А вот `Panels`,
которая метила в `ui.panels`, получала отказ — её место начинало место `Ui`.

## Занятое место переживает хендл

Дроп структуры её место не освобождает. Таблица принадлежит store и живёт
ровно столько же, сколько он.

Так задумано. Освобождай место дроп — и отказ зависел бы от того, когда
значение вышло из области видимости: одна и та же программа то открывалась бы
чисто, то падала, смотря по тому, где кончилась привязка `let`.

## Как спросить, у кого место

<!-- shown: asking who claimed a place -->
```rust
let field = StorePath::parse_joined("ui.panels.left.visible")?;
let owner = store.owners().declared_by(&field);

println!("{owner:?}");
```
<!-- /shown -->

Ищут по точному пути, а записаны те пути, которые заняты на самом деле:
собственный путь поля, а не префикс, под которым оно сидит. Поэтому `Ui` выше
находится по `ui.panels.left.visible`, а не по `ui`.

Отвечает он именем схемы — тем самым, которое печатает отказ.
