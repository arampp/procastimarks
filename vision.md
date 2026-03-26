# Procastimarks Vision

## The problem you want to solve

After a while of procastinating, I often have the feeling I have seen everything interesting on the internet.
In this situation I want to have a collection of interesting websites, I have seen earlier but did not have time to check them out.

When stumbling over an interesting website, I want to have an easy way to store this website. I don't want to have to open a new tab, go to my bookmark manager, add a new bookmark and fill out all the information. I want to have a one click solution to store the website.

Furthermore, I want to create a proof of concept if it is possible to create a full stack web application only in the Rust programming language.

## What the desired outcome looks like

I want to create a web application that helps me to organize my bookmarks. Everytime I stumble over an interesting website I want to have an easy way to store this website. Storing the website should be one click maybe on a bookmarklet in my favourite bar in the browser.

The web application should be independent from browsers and should be accessible publicly from the internet so that I can access it from various computers.

An entry of a website should consist of the following data:

- URL
- Title (automatically fetched from the website)
- Description (automatically fetched from the website)
- Tags (manually added by me)
- Date of creation (automatically set when the bookmark is created)
- Comment (manually added by me)

To find previously stored bookmarks, I need the following features:

- Full text search over title and description
- Filter by tags

Bookmarks must not be publicly accessible, that means we need some simple authentication mechanism to protect the bookmarks from unauthorized access.

It should have the look and feel of a vintage web application from the early 2000s, with a minimalistic design. It should be easy to use and should not have any unnecessary features.

One design principle should be "keep it simple". It should have simple mechanisms in the UI, but also be simple in the architecture and implementation.

## Constraints: budget, timeline, tech stack preferences, platform

- Programming Language: Rust for the backend and frontend.
- Backend Framework: Auxum Web
- Timeframe: one weekend for an MVP
- Budget: no budget, this is a personal project
- Hosting: self-hosted on a private server using Docker compose
- Number of users: only me
- UX Design: minimalistic, it should have a vintage style like early 2000s web applications

## What it is not: explicit boundaries help the AI avoid scope creep

- A bookmarks sharing application, I want to keep my bookmarks private
- A browser extension, I want to have a web application that is independent from the browser
- A mobile application, I want to have a web application that is accessible from various devices
- A read it later app.
- A knowledge management system, I just want to store bookmarks, I don't want to create a knowledge graph or something like that.

## Inspiration: similar tools or approaches you have seen

- del.icio.us, a social bookmarking web service for storing, sharing, and discovering web bookmarks (but skip the social part).
- raindrop.io, a bookmark manager that allows you to save and organize your bookmarks in a visually appealing way (but skip the visual part).