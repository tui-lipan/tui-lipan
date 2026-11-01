//! Container-specific expansion keeps debug stack frames local to the active widget kind.

use super::*;

impl ComponentRegistry {
    // Keep this dispatcher small. Moving every variant and its expansion temporaries into one
    // match gives debug builds a stack frame containing storage for all branches at every depth.
    pub(super) fn expand_element(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let expand: fn(
            &mut Self,
            &mut HostState,
            &mut ContainerPath,
            ExpandElementParams,
        ) -> Element = match &params.kind {
            ElementKind::VStack(_) => Self::expand_v_stack,
            ElementKind::HStack(_) => Self::expand_h_stack,
            ElementKind::Flow(_) => Self::expand_flow,
            ElementKind::ScrollView(_) => Self::expand_scroll_view,
            ElementKind::Splitter(_) => Self::expand_splitter,
            ElementKind::Grid(_) => Self::expand_grid,
            ElementKind::Canvas(_) => Self::expand_canvas,
            ElementKind::Frame(_) => Self::expand_frame,
            ElementKind::ZStack(_) => Self::expand_z_stack,
            ElementKind::Center(_) => Self::expand_center,
            ElementKind::CenterPin(_) => Self::expand_center_pin,
            ElementKind::StatusBarLayout(_) => Self::expand_status_bar_layout,
            ElementKind::Popover(_) => Self::expand_popover,
            ElementKind::Portal(_) => Self::expand_portal,
            ElementKind::Group(_) => Self::expand_group,
            ElementKind::ThemeProvider(_) => Self::expand_theme_provider,
            ElementKind::ContextProvider(_) => Self::expand_context_provider,
            ElementKind::Memo(_) => Self::expand_memo,
            ElementKind::EffectScope(_) => Self::expand_effect_scope,
            ElementKind::MouseRegion(_) => Self::expand_mouse_region,
            ElementKind::DragSource(_) => Self::expand_drag_source,
            ElementKind::DropTarget(_) => Self::expand_drop_target,
            ElementKind::Animated(_) => Self::expand_animated,
            ElementKind::Component(_) => Self::expand_component,
            _ => Self::expand_leaf,
        };
        expand(self, host, path, params)
    }

    fn expand_v_stack(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::VStack(mut vs) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        self.expand_vec_container(
            host,
            path,
            &mut vs.children,
            ExpandVecContainerParams {
                parent,
                key: key.clone(),
                tag: ContainerTag::VStack,
                index_in_parent,
                epoch,
                viewport,
            },
        );
        metadata.rebuild(key, pointer_focus, ElementKind::VStack(vs), layout)
    }

    fn expand_h_stack(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::HStack(mut hs) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        self.expand_vec_container(
            host,
            path,
            &mut hs.children,
            ExpandVecContainerParams {
                parent,
                key: key.clone(),
                tag: ContainerTag::HStack,
                index_in_parent,
                epoch,
                viewport,
            },
        );
        metadata.rebuild(key, pointer_focus, ElementKind::HStack(hs), layout)
    }

    fn expand_flow(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::Flow(mut flow) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        self.expand_vec_container(
            host,
            path,
            &mut flow.children,
            ExpandVecContainerParams {
                parent,
                key: key.clone(),
                tag: ContainerTag::Flow,
                index_in_parent,
                epoch,
                viewport,
            },
        );
        metadata.rebuild(key, pointer_focus, ElementKind::Flow(flow), layout)
    }

    fn expand_scroll_view(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::ScrollView(mut sv) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        self.expand_vec_container(
            host,
            path,
            &mut sv.children,
            ExpandVecContainerParams {
                parent,
                key: key.clone(),
                tag: ContainerTag::ScrollView,
                index_in_parent,
                epoch,
                viewport,
            },
        );
        metadata.rebuild(key, pointer_focus, ElementKind::ScrollView(sv), layout)
    }

    fn expand_splitter(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::Splitter(mut splitter) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        self.expand_vec_container(
            host,
            path,
            &mut splitter.children,
            ExpandVecContainerParams {
                parent,
                key: key.clone(),
                tag: ContainerTag::Splitter,
                index_in_parent,
                epoch,
                viewport,
            },
        );
        metadata.rebuild(key, pointer_focus, ElementKind::Splitter(splitter), layout)
    }

    fn expand_grid(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::Grid(mut grid) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        let placements: Vec<_> = grid.items.iter().map(|i| (i.placement, i.span)).collect();
        let mut children: Vec<Element> = std::mem::take(&mut grid.items)
            .into_iter()
            .map(|i| i.element)
            .collect();
        self.expand_vec_container(
            host,
            path,
            &mut children,
            ExpandVecContainerParams {
                parent,
                key: key.clone(),
                tag: ContainerTag::Grid,
                index_in_parent,
                epoch,
                viewport,
            },
        );
        grid.items = children
            .into_iter()
            .zip(placements)
            .map(|(element, (placement, span))| crate::widgets::GridItem {
                element,
                placement,
                span,
            })
            .collect();
        metadata.rebuild(key, pointer_focus, ElementKind::Grid(grid), layout)
    }

    fn expand_canvas(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::Canvas(mut canvas) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        let rects: Vec<_> = canvas.items.iter().map(|item| item.rect).collect();
        let mut children: Vec<Element> = std::mem::take(&mut canvas.items)
            .into_iter()
            .map(|item| item.element)
            .collect();
        self.expand_vec_container(
            host,
            path,
            &mut children,
            ExpandVecContainerParams {
                parent,
                key: key.clone(),
                tag: ContainerTag::Canvas,
                index_in_parent,
                epoch,
                viewport,
            },
        );
        canvas.items = children
            .into_iter()
            .zip(rects)
            .map(|(element, rect)| crate::widgets::CanvasItem { rect, element })
            .collect();
        metadata.rebuild(key, pointer_focus, ElementKind::Canvas(canvas), layout)
    }

    fn expand_frame(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::Frame(mut frame) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        let seg = PathSegment {
            tag: ContainerTag::Frame,
            id: segment_id(&key, index_in_parent),
        };
        path.push(seg);

        let header = frame.header.take().map(|c| *c);
        let child = frame.child.take().map(|c| *c);
        let has_header = header.is_some();
        let has_child = child.is_some();

        if has_header || has_child {
            let mut children = Vec::new();
            if let Some(header) = header {
                children.push(header);
            }
            if let Some(child) = child {
                children.push(child);
            }

            let expanded = self.expand_children(host, parent, path, children, epoch, viewport);
            let mut iter = expanded.into_iter();
            if has_header {
                frame.header = iter.next().map(Box::new);
            }
            if has_child {
                frame.child = iter.next().map(Box::new);
            }
        }

        path.pop();
        metadata.rebuild(key, pointer_focus, ElementKind::Frame(frame), layout)
    }

    fn expand_z_stack(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::ZStack(mut zs) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        self.expand_vec_container(
            host,
            path,
            &mut zs.children,
            ExpandVecContainerParams {
                parent,
                key: key.clone(),
                tag: ContainerTag::ZStack,
                index_in_parent,
                epoch,
                viewport,
            },
        );
        metadata.rebuild(key, pointer_focus, ElementKind::ZStack(zs), layout)
    }

    fn expand_center(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::Center(mut center) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        let seg = PathSegment {
            tag: ContainerTag::Center,
            id: segment_id(&key, index_in_parent),
        };
        path.push(seg);
        let child = center.child.take().map(|c| *c);
        if let Some(child) = child {
            let expanded = self.expand_single(host, parent, path, child, epoch, viewport);
            center.child = Some(Box::new(expanded));
        }
        path.pop();
        metadata.rebuild(key, pointer_focus, ElementKind::Center(center), layout)
    }

    fn expand_center_pin(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::CenterPin(mut cp) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        let seg = PathSegment {
            tag: ContainerTag::CenterPin,
            id: segment_id(&key, index_in_parent),
        };
        path.push(seg);
        let slots: Vec<Option<Box<crate::core::element::Element>>> =
            vec![cp.top.take(), cp.center.take(), cp.bottom.take()];
        let expanded: Vec<Option<crate::core::element::Element>> = slots
            .into_iter()
            .enumerate()
            .map(|(slot_idx, opt)| {
                opt.map(|boxed| {
                    path.push(PathSegment {
                        tag: ContainerTag::CenterPinSlot,
                        id: SegmentId::Index(slot_idx),
                    });
                    let mut children =
                        self.expand_children(host, parent, path, vec![*boxed], epoch, viewport);
                    path.pop();
                    children.pop().unwrap()
                })
            })
            .collect();
        let mut iter = expanded.into_iter();
        cp.top = iter.next().flatten().map(Box::new);
        cp.center = iter.next().flatten().map(Box::new);
        cp.bottom = iter.next().flatten().map(Box::new);
        path.pop();
        metadata.rebuild(key, pointer_focus, ElementKind::CenterPin(cp), layout)
    }

    fn expand_status_bar_layout(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::StatusBarLayout(mut status_layout) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        let seg = PathSegment {
            tag: ContainerTag::StatusBarLayout,
            id: segment_id(&key, index_in_parent),
        };
        path.push(seg);
        let slots = vec![
            status_layout.left,
            status_layout.center,
            status_layout.right,
        ];
        let mut expanded = slots
            .into_iter()
            .enumerate()
            .map(|(slot_idx, boxed)| {
                path.push(PathSegment {
                    tag: ContainerTag::StatusBarLayoutSlot,
                    id: SegmentId::Index(slot_idx),
                });
                let expanded = self.expand_single(host, parent, path, *boxed, epoch, viewport);
                path.pop();
                Box::new(expanded)
            })
            .collect::<Vec<_>>()
            .into_iter();
        status_layout.left = expanded.next().unwrap();
        status_layout.center = expanded.next().unwrap();
        status_layout.right = expanded.next().unwrap();
        path.pop();
        metadata.rebuild(
            key,
            pointer_focus,
            ElementKind::StatusBarLayout(status_layout),
            layout,
        )
    }

    fn expand_popover(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::Popover(mut popover) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        let seg = PathSegment {
            tag: ContainerTag::Popover,
            id: segment_id(&key, index_in_parent),
        };
        path.push(seg);
        let trigger = *popover.trigger;
        let content = *popover.content;
        let expanded =
            self.expand_children(host, parent, path, vec![trigger, content], epoch, viewport);
        let mut iter = expanded.into_iter();
        popover.trigger = Box::new(
            iter.next()
                .unwrap_or_else(|| crate::widgets::Text::new("").into()),
        );
        popover.content = Box::new(
            iter.next()
                .unwrap_or_else(|| crate::widgets::Text::new("").into()),
        );
        path.pop();
        metadata.rebuild(key, pointer_focus, ElementKind::Popover(popover), layout)
    }

    fn expand_portal(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::Portal(mut portal) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        let seg = PathSegment {
            tag: ContainerTag::Portal,
            id: segment_id(&key, index_in_parent),
        };
        path.push(seg);
        let child = *portal.content;
        portal.content = Box::new(self.expand_single(host, parent, path, child, epoch, viewport));
        path.pop();
        metadata.rebuild(key, pointer_focus, ElementKind::Portal(portal), layout)
    }

    fn expand_group(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::Group(mut group) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        let seg = PathSegment {
            tag: ContainerTag::Group,
            id: segment_id(&key, index_in_parent),
        };
        path.push(seg);

        let child = *group.child;
        group.child = Box::new(self.expand_single(host, parent, path, child, epoch, viewport));

        path.pop();
        metadata.rebuild(key, pointer_focus, ElementKind::Group(group), layout)
    }

    fn expand_theme_provider(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::ThemeProvider(tp) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        let tp = *tp;
        let seg = PathSegment {
            tag: ContainerTag::ThemeProvider,
            id: segment_id(&key, index_in_parent),
        };
        path.push(seg);
        self.theme_stack.push(tp.theme.clone());
        let mut child = self.expand_single(host, parent, path, tp.child, epoch, viewport);
        self.theme_stack.pop();
        path.pop();
        child.pointer_focus &= pointer_focus;
        #[cfg(feature = "profiling-tracing")]
        let theme_start = web_time::Instant::now();
        let child = crate::style::apply_document_theme_carve_out(&tp.theme, child);
        #[cfg(feature = "profiling-tracing")]
        tracing::trace!(
            target: "tui_lipan::perf",
            apply_theme_ms = theme_start.elapsed().as_secs_f64() * 1000.0,
        );
        metadata.rebuild(
            key,
            pointer_focus,
            ElementKind::ThemeProvider(Box::new(crate::core::element::ThemeProviderElement {
                theme: tp.theme,
                child,
            })),
            layout,
        )
    }

    fn expand_context_provider(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout: _,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::ContextProvider(cp) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        let cp = *cp;
        let seg = PathSegment {
            tag: ContainerTag::ContextProvider,
            id: segment_id(&key, index_in_parent),
        };
        path.push(seg);

        let prev = host.prev_context(path).cloned();
        let generation = if let Some(prev) = &prev {
            if prev.type_id == cp.type_id && (cp.equals)(prev.value.as_ref(), cp.value.as_ref()) {
                prev.generation
            } else {
                self.context_generation = self.context_generation.wrapping_add(1).max(1);
                self.context_generation
            }
        } else {
            self.context_generation = self.context_generation.wrapping_add(1).max(1);
            self.context_generation
        };

        let mut provider = cp.clone();
        provider.generation = generation;
        host.set_next_context(
            path,
            host::ContextProviderRecord {
                type_id: provider.type_id,
                value: Arc::clone(&provider.value),
                generation,
            },
        );

        self.context_stack.push(provider.clone());
        let mut child = self.expand_single(host, parent, path, provider.child, epoch, viewport);
        self.context_stack.pop();
        path.pop();
        child.pointer_focus &= pointer_focus;
        metadata.apply_to(child)
    }

    fn expand_memo(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout: _,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::Memo(memo) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        let seg = PathSegment {
            tag: ContainerTag::Memo,
            id: segment_id(&key, index_in_parent),
        };
        path.push(seg);

        // Probe deps_hash without cloning; only deep-clone the cached
        // subtree on a true hit (deps match).
        let hit = host
            .prev_memo(path, memo.call_site)
            .is_some_and(|cached| cached.deps_hash == memo.deps_hash);
        let mut child = if hit {
            let (mut expanded_child, descendant_ids) = host
                .prev_memo(path, memo.call_site)
                .map(|cached| (cached.expanded_child.clone(), cached.descendant_ids.clone()))
                .expect("hit implies a previous memo entry");
            let needs_refresh = descendant_ids
                .iter()
                .copied()
                .any(|id| self.component_subtree_needs_refresh(id));
            if needs_refresh {
                let descendant_ids =
                    collect_component_ids_in_element(&expanded_child, &self.scope_to_id);
                for child_id in descendant_ids {
                    if !self.component_subtree_needs_refresh(child_id) {
                        continue;
                    }
                    let replacement = self.expand_component_instance(child_id, epoch, viewport);
                    let scope = self.arena.get(child_id).scope;
                    let mut replacement = Some(replacement);
                    if expanded_child.replace_group_child_by_scope(scope, &mut replacement) {
                        continue;
                    } else {
                        #[cfg(feature = "devtools")]
                        record_memo_miss(MemoMissReason::ViewMemoStructureChanged);
                        let rebuilt = (memo.builder)();
                        let mut expanded = self.expand_children(
                            host,
                            parent,
                            path,
                            vec![rebuilt],
                            epoch,
                            viewport,
                        );
                        let mut child = expanded
                            .pop()
                            .unwrap_or_else(|| crate::widgets::Text::new("").into());
                        let descendant_ids =
                            collect_component_ids_in_element(&child, &self.scope_to_id);
                        host.set_next_memo(
                            path,
                            memo.call_site,
                            MemoCacheEntry {
                                deps_hash: memo.deps_hash,
                                expanded_child: child.clone(),
                                descendant_ids,
                            },
                        );
                        path.pop();
                        child.pointer_focus &= pointer_focus;
                        return metadata.apply_to(child);
                    }
                }
            }
            #[cfg(feature = "devtools")]
            increment_memo_hit_counter();
            expanded_child
        } else if host.prev_memo(path, memo.call_site).is_some() {
            #[cfg(feature = "devtools")]
            record_memo_miss(MemoMissReason::ViewMemoDepsChanged);
            let rebuilt = (memo.builder)();
            self.expand_single(host, parent, path, rebuilt, epoch, viewport)
        } else {
            #[cfg(feature = "devtools")]
            record_memo_miss(MemoMissReason::ViewMemoNoCache);
            let rebuilt = (memo.builder)();
            self.expand_single(host, parent, path, rebuilt, epoch, viewport)
        };

        let descendant_ids = collect_component_ids_in_element(&child, &self.scope_to_id);
        host.set_next_memo(
            path,
            memo.call_site,
            MemoCacheEntry {
                deps_hash: memo.deps_hash,
                expanded_child: child.clone(),
                descendant_ids,
            },
        );
        path.pop();
        child.pointer_focus &= pointer_focus;
        metadata.apply_to(child)
    }

    fn expand_effect_scope(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::EffectScope(mut scope) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        let seg = PathSegment {
            tag: ContainerTag::EffectScope,
            id: segment_id(&key, index_in_parent),
        };
        path.push(seg);
        if let Some(child) = scope.child.take() {
            let expanded = self.expand_single(host, parent, path, *child, epoch, viewport);
            scope.child = Some(Box::new(expanded));
        }
        path.pop();
        metadata.rebuild(key, pointer_focus, ElementKind::EffectScope(scope), layout)
    }

    fn expand_mouse_region(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::MouseRegion(mut region) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        let seg = PathSegment {
            tag: ContainerTag::MouseRegion,
            id: segment_id(&key, index_in_parent),
        };
        path.push(seg);
        if let Some(child) = region.child.take() {
            let expanded = self.expand_single(host, parent, path, *child, epoch, viewport);
            region.child = Some(Box::new(expanded));
        }
        path.pop();
        metadata.rebuild(key, pointer_focus, ElementKind::MouseRegion(region), layout)
    }

    fn expand_drag_source(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::DragSource(mut source) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        let seg = PathSegment {
            tag: ContainerTag::DragSource,
            id: segment_id(&key, index_in_parent),
        };
        path.push(seg);
        if let Some(child) = source.child.take() {
            let expanded = self.expand_single(host, parent, path, *child, epoch, viewport);
            source.child = Some(Box::new(expanded));
        }
        path.pop();
        metadata.rebuild(key, pointer_focus, ElementKind::DragSource(source), layout)
    }

    fn expand_drop_target(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::DropTarget(mut target) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        let seg = PathSegment {
            tag: ContainerTag::DropTarget,
            id: segment_id(&key, index_in_parent),
        };
        path.push(seg);
        if let Some(child) = target.child.take() {
            let expanded = self.expand_single(host, parent, path, *child, epoch, viewport);
            target.child = Some(Box::new(expanded));
        }
        path.pop();
        metadata.rebuild(key, pointer_focus, ElementKind::DropTarget(target), layout)
    }

    fn expand_animated(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let ElementKind::Animated(mut animated) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        let seg = PathSegment {
            tag: ContainerTag::Animated,
            id: segment_id(&key, index_in_parent),
        };
        path.push(seg);
        animated.child = if animated
            .visibility
            .as_ref()
            .is_some_and(|(visible, _)| !visible)
        {
            Box::new(crate::widgets::Spacer::new().into())
        } else {
            Box::new(self.expand_single(host, parent, path, *animated.child, epoch, viewport))
        };
        path.pop();
        metadata.rebuild(key, pointer_focus, ElementKind::Animated(animated), layout)
    }

    fn expand_component(
        &mut self,
        _host: &mut HostState,
        _path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            ..
        } = params;
        let ElementKind::Component(component) = kind else {
            unreachable!("expansion dispatch selects the matching kind")
        };
        debug_assert!(
            false,
            "component elements must be expanded in parent context"
        );
        metadata.rebuild(
            key,
            pointer_focus,
            ElementKind::Component(component),
            layout,
        )
    }

    fn expand_leaf(
        &mut self,
        _host: &mut HostState,
        _path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            key,
            metadata,
            pointer_focus,
            layout,
            kind,
            ..
        } = params;
        let other = kind;
        metadata.rebuild(key, pointer_focus, other, layout)
    }
}
