---
title: "Meet the bike shop: a whole business built with Renox"
description: Three stores that sell, rent and service bikes, with staff roles per store and reports. Renox's flagship example, live and explained.
date: 2026-10-07
updated: 2026-10-08
author: Arif Rachim
tags: examples, showcase
---
Framework examples are usually small. A to-do list proves that a form can be saved. It doesn't prove that the framework holds up when a real business with real rules is built on it. So the flagship example of Renox is a business: [a bike shop with three stores](https://bikeshop.renox.rs).

## What it does

The shop **sells** bikes, gear and parts. It has a catalogue with filters and search, product pages with variants and stock per store, a cart and a checkout, plus a point of sale for the counter.

It **rents** bikes by the hour or the day. A timeline shows which bike is free when. Deposits are taken, a late return is charged, and a bike placed at another store is booked there.

It **services** bikes. Customers book a slot in the workshop, mechanics work through a board, and customers approve quotes. Service plans book the visits for you every week or month.

Behind the counter it **runs three stores**:

- staff have roles in a given store, for a given time (a mechanic who helps another store this week);
- stock is a ledger, and there are consignment goods and purchase orders;
- the stores keep books between them, with a monthly settlement;
- there are reports and exports, and an admin panel.

## Every page explains itself

Each page has an **About this page** panel. Since Renox 1.0, it sits beside the page on a wide screen, with the code behind it: the handler, the template, the query, cut from the source when the app is built. It says what the page is for, who uses it, which Renox features it relies on and why, what happens under the hood, which guides to read and which source files to open. If you want to see how Renox does route model binding, rate limits, roles per tenant, live validation or a data grid with exports, there is a page that does it, and it tells you so.

To try every side of the shop, log in with one of the demo accounts listed on [the login page](https://bikeshop.renox.rs/login): a customer, the owner, a manager, a cashier or a mechanic.

## What it is built with

It uses Renox and Renox's plugins, and that's all:

- **The UI kit** for every page. The storefront has a warm editorial theme set through the kit's own tokens, so the components follow it in light and dark mode.
- **The data grid** for the staff lists, with filters, summaries, exports to CSV and Excel, and a print page.
- **Roles per store**, so staff rights depend on the store they are working in. The code checks permissions, never role names.
- **renox-2fa** for staff sign-in, **renox-billing** for service plans, **renox-admin** for the admin panel.
- **The queue and the scheduler**: notifications, overdue rentals, monthly settlements.
- **One binary** to deploy. The live demo is built by GitHub Actions, and the server pulls each new build.

## Read the code

The bike shop lives in the repository at [examples/bikeshop](https://github.com/renox-rs/renox/tree/main/examples/bikeshop). Its README maps every feature to its page and its files. Start the app with `rnx serve`, open any page, and press **About this page**.
