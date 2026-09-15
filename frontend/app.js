        const { createApp, ref, computed, watch, nextTick, onMounted, onUnmounted } = Vue

        createApp({
            setup() {
                const newNoteTitle = ref('')
                const newNotePriority = ref('medium')
                const newNoteDueDate = ref('')
                const newNoteRepeatRule = ref('')
                const newNoteStatus = ref('backlog')
                const newNoteContent = ref('')
                const newNoteChecklist = ref([])
                const newNoteStepText = ref('')
                const newNoteDepartment = ref('')
                const newNoteDepartments = ref([])
                const newNoteDeptInput = ref('')
                const newNoteAppointments = ref([])
                const newAptTitle = ref('')
                const newAptStart = ref('')
                const newAptEnd = ref('')
                const newAptTime = ref('')
                const newAptEndTime = ref('')
                const newAptHasEnd = ref(false)
                const isNewNoteOpen = ref(false)
                const shortcutHelpOpen = ref(false)
                const confirmOpen = ref(false)
                const confirmMessage = ref('')
                const confirmOkLabel = ref('')
                let confirmAction = null
                const requestConfirm = (msg, action, okLabel = t('common.delete')) => {
                    confirmMessage.value = msg
                    confirmOkLabel.value = okLabel
                    confirmAction = action || null
                    confirmOpen.value = true
                }
                const confirmAccept = () => {
                    const a = confirmAction
                    confirmOpen.value = false
                    confirmAction = null
                    if (a) {
                        const r = a()
                        if (r && r.catch) r.catch(() => {})
                    }
                }
                const confirmCancel = () => {
                    confirmOpen.value = false
                    confirmAction = null
                }
                const draftTs = ref(null)
                const draftRestored = ref(false)
                const newNoteTitleInputRef = ref(null)
                const imageUploadInputRef = ref(null)
                const newNoteTextareaRef = ref(null)
                const newNoteCaretMirrorRef = ref(null)
                const newNoteStepInputRef = ref(null)
                const draggedNote = ref(null)
                const isBacklogCollapsed = ref(true)
                const isInProgressCollapsed = ref(false)
                const isReviewCollapsed = ref(false)
                const isDoneCollapsed = ref(false)
                const isArchiveCollapsed = ref(true)
                const searchQuery = ref('')
                const searchInputRef = ref(null)
                const textareaRef = ref(null)
                const caretMirrorRef = ref(null)
                const wikiTextareaRef = ref(null)
                const wikiCaretMirrorRef = ref(null)

                const isModalOpen = ref(false)
                const isPreviewMode = ref(false)
                const newNotePreviewMode = ref(false)
                const newNoteMarkdownMax = ref(false)
                const detailMarkdownMax = ref(false)
                const activeNote = ref(null)
                const activeChecklist = ref([])
                const newStepText = ref('')
                const activeNoteAppointments = ref([])
                const activeNoteDepartments = ref([])
                const modalDeptInput = ref('')
                const modalAptTitle = ref('')
                const modalAptStart = ref('')
                const modalAptEnd = ref('')
                const modalAptTime = ref('')
                const modalAptEndTime = ref('')
                const modalAptHasEnd = ref(false)

                // Views / navigation
                const activeView = ref((localStorage.getItem('notice-view') === 'addressbook' ? 'data' : localStorage.getItem('notice-view')) || 'dashboard')
                watch(activeView, (v) => localStorage.setItem('notice-view', v))
                const calViewMode = ref('month')
                const calCursor = ref(new Date())
                const calSelectedDay = ref(null)

                // Autocomplete state for [[ note links and {{ address links
                const showAutocomplete = ref(false)
                const autocompleteType = ref('note')
                const autocompleteResults = ref([])
                const autocompleteIndex = ref(0)
                const autocompleteQuery = ref('')
                const autocompleteStart = ref(0)
                const autocompletePos = ref({ x: 0, y: 0, flip: false })
                let autocompleteTimer = null
                const autocompleteContainerRef = ref(null)
                const exportMenuOpen = ref(false)
                const mobileMenuOpen = ref(false)
                const importFileInputRef = ref(null)
                const importMode = ref('merge')
                const trashNotes = ref([])
                const trashContacts = ref([])

                // --- Server config (config.toml) injected at serve time ---
                const appInit = (window.__INITIAL_CONFIG__ && window.__INITIAL_CONFIG__.config) || {}
                const createdFresh = !!(window.__INITIAL_CONFIG__ && window.__INITIAL_CONFIG__.created_fresh)
                const appInitApp = appInit.app || {}

                // Theme (dark/light)
                const isDark = ref((appInitApp.theme || 'dark') !== 'light')
                const toggleTheme = () => {
                    isDark.value = !isDark.value
                    document.documentElement.classList.toggle('dark', isDark.value)
                    saveConfig({ app: { theme: isDark.value ? 'dark' : 'light' } })
                }

                // Settings modal (auto-archive + trash purge)
                const isSettingsOpen = ref(false)

                // Settings (shared apply + persistence via config.toml)
                const autoArchiveEnabled = ref(!!appInitApp.auto_archive_enabled)
                const autoArchiveDay = ref(appInitApp.auto_archive_day || 1)
                const trashPurgeEnabled = ref(!!appInitApp.trash_purge_enabled)
                const trashPurgeDay = ref(appInitApp.trash_purge_day ?? 30)
                const networkOpen = ref((appInit.server && appInit.server.host === '0.0.0.0') || false)
                const applyConfig = (cfg) => {
                    if (!cfg || !cfg.app) return
                    const a = cfg.app
                    if (a.theme) isDark.value = a.theme !== 'light'
                    if (a.auto_archive_enabled !== undefined) autoArchiveEnabled.value = !!a.auto_archive_enabled
                    if (a.auto_archive_day) autoArchiveDay.value = a.auto_archive_day
                    if (a.trash_purge_enabled !== undefined) trashPurgeEnabled.value = !!a.trash_purge_enabled
                    if (a.trash_purge_day !== undefined) trashPurgeDay.value = a.trash_purge_day
                    if (a.language && window.__I18N__[a.language]) {
                        language.value = a.language
                        document.documentElement.lang = a.language
                    }
                    document.documentElement.classList.toggle('dark', isDark.value)
                }
                const toggleNetworkOpen = async () => {
                    networkOpen.value = !networkOpen.value
                    await saveConfig({ server: { host: networkOpen.value ? '0.0.0.0' : '127.0.0.1' } })
                }
                const saveConfig = async (patch) => {
                    try {
                        const res = await fetch('/api/config', {
                            method: 'PUT',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify(patch)
                        })
                        if (res.ok) {
                            const data = await res.json()
                            if (data.config) applyConfig(data.config)
                        }
                    } catch (e) {
                        console.error('Fehler beim Speichern der Einstellungen', e)
                    }
                }
                watch([autoArchiveEnabled, autoArchiveDay], () => {
                    saveConfig({ app: { auto_archive_enabled: autoArchiveEnabled.value, auto_archive_day: autoArchiveDay.value || 1 } })
                })
                watch([trashPurgeEnabled, trashPurgeDay], () => {
                    saveConfig({ app: { trash_purge_enabled: trashPurgeEnabled.value, trash_purge_day: trashPurgeDay.value ?? 30 } })
                })
                const adoptLegacySettings = () => {
                    if (!createdFresh) return
                    const pApp = {}
                    try {
                        const theme = localStorage.getItem('notice-theme')
                        if (theme === 'dark' || theme === 'light') pApp.theme = theme
                        if (localStorage.getItem('notice-autoarchive-enabled') !== null) pApp.auto_archive_enabled = localStorage.getItem('notice-autoarchive-enabled') === '1'
                        if (localStorage.getItem('notice-autoarchive-day') !== null) pApp.auto_archive_day = parseInt(localStorage.getItem('notice-autoarchive-day') || '1', 10)
                        if (localStorage.getItem('notice-trashpurge-enabled') !== null) pApp.trash_purge_enabled = localStorage.getItem('notice-trashpurge-enabled') === '1'
                        if (localStorage.getItem('notice-trashpurge-day') !== null) pApp.trash_purge_day = parseInt(localStorage.getItem('notice-trashpurge-day') ?? '30', 10)
                    } catch (e) {}
                    if (Object.keys(pApp).length) saveConfig({ app: pApp })
                    try {
                        localStorage.removeItem('notice-theme')
                        localStorage.removeItem('notice-autoarchive-enabled')
                        localStorage.removeItem('notice-autoarchive-day')
                        localStorage.removeItem('notice-trashpurge-enabled')
                        localStorage.removeItem('notice-trashpurge-day')
                    } catch (e) {}
                }

                // --- i18n (server-injected dictionaries, see window.__I18N__) ---
                const LOCALES = { de: 'de-DE', en: 'en-US', es: 'es-ES', fr: 'fr-FR' }
                const language = ref(appInitApp.language || 'de')
                const t = (key, params) => {
                    const all = window.__I18N__ || {}
                    const dict = all[language.value] || {}
                    let s = dict[key]
                    if (s === undefined) {
                        const deDict = all.de || {}
                        s = deDict[key] !== undefined ? deDict[key] : key
                    }
                    if (s !== null && typeof s === 'object' && params && params.count !== undefined) {
                        s = (params.count === 1 ? s.one : s.other)
                        if (s === undefined) s = key
                    }
                    if (params && s !== null && typeof s !== 'object') {
                        for (const k of Object.keys(params)) {
                            s = String(s).split('{' + k + '}').join(String(params[k]))
                        }
                    }
                    return s
                }
                const locale = () => LOCALES[language.value] || 'de-DE'
                const switchLanguage = async (lang) => {
                    if (!((window.__I18N__ || {})[lang])) return
                    language.value = lang
                    document.documentElement.lang = lang
                    saveConfig({ app: { language: lang } })
                }
                watch(language, (lang) => {
                    document.documentElement.lang = lang
                })
                adoptLegacySettings()
                const trashPurging = ref(false)
                const purgeTrash = async (days) => {
                    if (trashPurging.value) return
                    trashPurging.value = true
                    try {
                        const res = await fetch('/api/trash/purge', {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({ days: days || 0 })
                        })
                        if (res.ok) await fetchTrash()
                    } catch (e) {
                        console.error('Fehler beim Purgen des Papierkorbs', e)
                    } finally {
                        trashPurging.value = false
                    }
                }
                const runTrashPurgeNow = () => {
                    requestConfirm(t('confirm.purge_trash'), () => purgeTrash(0), t('confirm.clean_label'))
                }

                // Import preview
                const pendingImport = ref(null)
                const existingCount = ref(0)
                const existingContactCount = ref(0)
                const cancelImport = () => { pendingImport.value = null }

                // Undo (single slot)
                const undoSlot = ref(null)
                const undoActive = ref(false)
                let undoTimeout = null
                const pushUndo = (msg, action) => {
                    undoSlot.value = { msg, action }
                    undoActive.value = true
                    clearTimeout(undoTimeout)
                    undoTimeout = setTimeout(() => { undoSlot.value = null; undoActive.value = false }, 10000)
                }
                const performUndo = async () => {
                    if (!undoSlot.value) return
                    undoActive.value = false
                    clearTimeout(undoTimeout)
                    const action = undoSlot.value.action
                    undoSlot.value = null
                    try { await action() } catch (e) { console.error('Fehler beim Rückgängig-machen', e) }
                }

                // Department suggestions (for new note form & detail modal)
                const departments = ref([])
                const newDeptName = ref('')
                const deptError = ref('')
                const deptDropdownOpen = ref(false)
                const deptDropdownSource = ref('')
                const deptIndex = ref(0)
                const newNoteDepartmentInputRef = ref(null)
                const modalDepartmentInputRef = ref(null)
                const contactDeptInput = ref('')
                const contactDepartmentInputRef = ref(null)

                const contextMenu = ref({
                    show: false,
                    x: 0,
                    y: 0,
                    source: 'edit',
                    selectionStart: 0,
                    selectionEnd: 0
                })

                const contactPopover = ref({
                    show: false,
                    x: 0,
                    y: 0,
                    contact: null
                })

                const columns = [
                    { id: 'backlog', title: 'status.backlog' },
                    { id: 'in_progress', title: 'status.in_progress' },
                    { id: 'review', title: 'status.review' },
                    { id: 'done', title: 'status.done' }
                ]

                const statusTitle = (id) => t('status.' + id)

                const collapsedMap = {
                    backlog: isBacklogCollapsed,
                    in_progress: isInProgressCollapsed,
                    review: isReviewCollapsed,
                    done: isDoneCollapsed
                }

                const isCollapsed = (colId) => collapsedMap[colId] ? collapsedMap[colId].value : false

                const collapseColumn = (colId) => {
                    if (collapsedMap[colId]) collapsedMap[colId].value = true
                }
                const expandColumn = (colId) => {
                    if (collapsedMap[colId]) collapsedMap[colId].value = false
                }
                const toggleCollapse = (colId) => {
                    if (isCollapsed(colId)) expandColumn(colId)
                    else collapseColumn(colId)
                }

                const collapsedStatuses = computed(() => columns.filter(c => isCollapsed(c.id)))
                const expandedColumns = computed(() => columns.filter(c => !isCollapsed(c.id)))

                const gridColsStyle = computed(() => {
                    const parts = expandedColumns.value.map(() => 'minmax(200px, 1fr)')
                    return parts.join(' ')
                })

                // --- Mobile board (status pills + single column + wheel menu) ---
                const mobileBoardColumn = ref('backlog')
                const wheelMenuOpen = ref(false)
                const wheelNoteData = ref(null)
                const wheelCenter = ref({ x: 0, y: 0 })
                const mobileDraggedNote = ref(null)
                const mobileDragInsertY = ref(null)
                let wheelTimer = null
                let wheelLongPress = false
                let touchStartX = 0
                let touchStartY = 0
                let touchStartTime = 0
                let touchStartNote = null
                let mobileLastY = 0
                let mobileDragging = false
                let suppressTouchMove = null
                const wheelStart = (e, note) => {
                    wheelLongPress = false
                    mobileDragging = false
                    touchStartNote = note
                    touchStartX = e.touches ? e.touches[0].clientX : e.clientX
                    touchStartY = e.touches ? e.touches[0].clientY : e.clientY
                    touchStartTime = Date.now()
                    mobileLastY = touchStartY
                    const el = e.currentTarget
                    if (wheelTimer) clearTimeout(wheelTimer)
                    wheelTimer = setTimeout(() => {
                        wheelLongPress = true
                        const rect = el.getBoundingClientRect()
                        wheelCenter.value = { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }
                        wheelNoteData.value = note
                        wheelMenuOpen.value = true
                    }, 500)
                }
                const wheelCancel = () => {
                    if (wheelTimer) clearTimeout(wheelTimer)
                    wheelTimer = null
                }
                const mobileCardClick = (note) => {
                    if (wheelLongPress) {
                        wheelLongPress = false
                        return
                    }
                    openModal(note)
                }
                const updateMobileDrop = (y) => {
                    const cont = document.querySelector('.mobile-board-scroll')
                    mobileDragInsertY.value = null
                    if (!cont) return
                    const draggedId = mobileDraggedNote.value && mobileDraggedNote.value.id
                    const contRect = cont.getBoundingClientRect()
                    const scroll = cont.scrollTop || 0
                    const cards = [...cont.querySelectorAll('.mobile-board-card')]
                        .filter(c => Number(c.dataset.noteId) !== draggedId)
                    const viewRects = cards.map(c => c.getBoundingClientRect())
                    let idx = viewRects.length
                    for (let i = 0; i < viewRects.length; i++) {
                        if (y < viewRects[i].top + viewRects[i].height / 2) { idx = i; break }
                    }
                    const toLocal = (rv) => rv.top - contRect.top + scroll
                    if (viewRects.length > 0) {
                        if (idx === 0) mobileDragInsertY.value = Math.max(0, toLocal(viewRects[0]) - 3)
                        else if (idx >= viewRects.length) mobileDragInsertY.value = toLocal(viewRects[viewRects.length - 1]) + viewRects[viewRects.length - 1].height + 3
                        else mobileDragInsertY.value = (toLocal(viewRects[idx - 1]) + viewRects[idx - 1].height + toLocal(viewRects[idx])) / 2
                    }
                    if (mobileDraggedNote.value) mobileDraggedNote.value._dropIdx = idx
                }
                const finalizeMobileDrop = async () => {
                    const note = mobileDraggedNote.value
                    const idx = note && note._dropIdx
                    if (!note || idx === undefined || idx === null || note.status !== mobileBoardColumn.value) return
                    const snapshot = { id: note.id, title: note.title, status: note.status, sort_order: note.sort_order }
                    mobileDragInsertY.value = null
                    await reorderColumn(mobileBoardColumn.value, note, idx)
                    pushUndo(t('undo.moved', { title: snapshot.title }), async () => {
                        const n = notes.value.find(x => x.id === snapshot.id)
                        if (!n) return
                        n.status = snapshot.status
                        n.sort_order = snapshot.sort_order
                        try {
                            await fetch(`/api/notes/${snapshot.id}`, {
                                method: 'PUT',
                                headers: { 'Content-Type': 'application/json' },
                                body: JSON.stringify({
                                    title: n.title,
                                    content: n.content,
                                    status: snapshot.status,
                                    priority: n.priority,
                                    due_date: n.due_date,
                                    sort_order: snapshot.sort_order
                                })
                            })
                        } catch (err) {
                            console.error('Fehler beim Rückgängig-machen (Verschieben)', err)
                        }
                    })
                }
                const onMobileTouchMove = (e) => {
                    const t = e.touches[0]
                    if (!t) return
                    mobileLastY = t.clientY
                    if (mobileDragging) return
                    if (wheelTimer) { clearTimeout(wheelTimer); wheelTimer = null }
                    if (wheelMenuOpen.value) return
                    const moved = Math.abs(t.clientY - touchStartY) + Math.abs(t.clientX - touchStartX)
                    const held = Date.now() - touchStartTime
                    if (held >= 300 && moved > 14 && touchStartNote) {
                        wheelLongPress = true
                        mobileDragging = true
                        mobileDraggedNote.value = touchStartNote
                        if (!suppressTouchMove) {
                            suppressTouchMove = (ev) => {
                                ev.preventDefault()
                                const cont = document.querySelector('.mobile-board-scroll')
                                if (cont) {
                                    const r = cont.getBoundingClientRect()
                                    const y = ev.touches ? ev.touches[0].clientY : ev.clientY
                                    if (y < r.top + 70) cont.scrollTop -= 12
                                    else if (y > r.bottom - 70) cont.scrollTop += 12
                                }
                            }
                            document.addEventListener('touchmove', suppressTouchMove, { passive: false })
                        }
                        updateMobileDrop(mobileLastY)
                    }
                }
                const onMobileTouchEnd = async () => {
                    if (wheelTimer) { clearTimeout(wheelTimer); wheelTimer = null }
                    if (suppressTouchMove) {
                        document.removeEventListener('touchmove', suppressTouchMove, { passive: false })
                        suppressTouchMove = null
                    }
                    if (mobileDragging) {
                        mobileDragging = false
                        wheelLongPress = true
                        dragCol.value = null
                        await finalizeMobileDrop()
                        mobileDraggedNote.value = null
                        mobileDragInsertY.value = null
                    }
                }
                const wheelSelect = async (target) => {
                    wheelMenuOpen.value = false
                    wheelLongPress = false
                    const n = wheelNoteData.value
                    wheelNoteData.value = null
                    if (!n) return
                    if (target === 'archive') { await archiveNote(n); return }
                    await updateNoteStatus(n, target)
                }
                const wheelButtonStyle = (index) => {
                    const total = 5
                    const angle = (index * (360 / total)) - 90
                    const rad = (angle * Math.PI) / 180
                    const x = Math.round(Math.cos(rad) * 78)
                    const y = Math.round(Math.sin(rad) * 78)
                    return { transform: `translate(${x}px, ${y}px)`, left: '50%', top: '50%' }
                }

                const notes = ref([])
                const notesById = computed(() => Object.fromEntries(notes.value.map(n => [n.id, n])))

                const fetchNotes = async () => {
                    try {
                        const res = await fetch('/api/notes')
                        if (res.ok) {
                            notes.value = await res.json()
                            runStartupChecks()
                        }
                    } catch (e) {
                        console.error('Fehler beim Laden der Notizen', e)
                    }
                }

                const archivedNotes = ref([])
                const fetchArchivedNotes = async () => {
                    try {
                        const res = await fetch('/api/archive')
                        if (res.ok) archivedNotes.value = await res.json()
                    } catch (e) {
                        console.error('Fehler beim Laden des Archivs', e)
                    }
                }
                const restoreFromArchive = async (id) => {
                    try {
                        await fetch(`/api/archive/${id}/restore`, { method: 'POST' })
                        await fetchArchivedNotes()
                        await fetchNotes()
                        await fetchDepartments()
                    } catch (e) { console.error('Fehler beim Archiv-Rückgängig', e) }
                }
                const hardDeleteArchived = (id) => {
                    requestConfirm(t('confirm.delete_note_force'), async () => {
                        try {
                            await fetch(`/api/archive/${id}`, { method: 'DELETE' })
                            await fetchArchivedNotes()
                        } catch (e) { console.error('Fehler beim Archiv-Löschen', e) }
                    })
                }

                const fetchDepartments = async () => {
                    try {
                        const res = await fetch('/api/departments')
                        if (res.ok) {
                            departments.value = await res.json()
                        }
                    } catch (e) {
                        console.error('Fehler beim Laden der Departments', e)
                    }
                }

                // --- Address book (contacts) ---
                const contacts = ref([])
                const contactFilter = ref('')
                const contactModalOpen = ref(false)
                const contactForm = ref({ id: null, name: '', title: '', department: '', departments: [], phone: '', mobile: '', fax: '', email: '', description: '' })
                const highlightContactId = ref(null)
                watch(activeView, (v) => {
                    if (v !== 'data') highlightContactId.value = null
                })

                const getContactDepartments = (contact) => {
                    try {
                        const raw = contact.departments
                        if (raw && Array.isArray(raw)) return raw.slice()
                        if (raw && typeof raw === 'string' && raw.trim() && raw.trim() !== '[]') {
                            const parsed = JSON.parse(raw)
                            if (Array.isArray(parsed)) return parsed.filter(s => s && s.trim())
                        }
                    } catch (e) { /* ignore malformed JSON */ }
                    if (contact.department && contact.department.trim()) return [contact.department.trim()]
                    return []
                }

                const filteredContacts = computed(() => {
                    const q = contactFilter.value.trim().toLowerCase()
                    if (!q) return contacts.value
                    return contacts.value.filter(c =>
                        (c.name || '').toLowerCase().includes(q) ||
                        (c.title || '').toLowerCase().includes(q) ||
                        getContactDepartments(c).some(d => d.toLowerCase().includes(q)) ||
                        (c.phone || '').toLowerCase().includes(q) ||
                        (c.mobile || '').toLowerCase().includes(q) ||
                        (c.fax || '').toLowerCase().includes(q) ||
                        (c.email || '').toLowerCase().includes(q)
                    )
                })

                const fetchContacts = async () => {
                    try {
                        const res = await fetch('/api/contacts')
                        if (res.ok) contacts.value = await res.json()
                    } catch (e) {
                        console.error('Fehler beim Laden der Kontakte', e)
                    }
                }

                const openContactModal = (contact) => {
                    if (contact && contact.id) {
                        contactForm.value = { ...contact, departments: getContactDepartments(contact) }
                    } else {
                        contactForm.value = { id: null, name: '', title: '', department: '', departments: [], phone: '', mobile: '', fax: '', email: '', description: '' }
                    }
                    contactDeptInput.value = ''
                    contactModalOpen.value = true
                }

                const saveContact = async () => {
                    if (!contactForm.value.name.trim()) return
                    const f = contactForm.value
                    const url = f.id ? `/api/contacts/${f.id}` : '/api/contacts'
                    const method = f.id ? 'PUT' : 'POST'
                    try {
                        const res = await fetch(url, {
                            method,
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({
                                name: f.name,
                                title: f.title || null,
                                department: f.departments.length ? f.departments[0] : (f.department || null),
                                departments: JSON.stringify(f.departments || []),
                                phone: f.phone || null,
                                mobile: f.mobile || null,
                                fax: f.fax || null,
                                email: f.email || null,
                                description: f.description || null
                            })
                        })
                        if (res.ok) {
                            contactModalOpen.value = false
                            await fetchContacts()
                        }
                    } catch (e) {
                        console.error('Fehler beim Speichern des Kontakts', e)
                    }
                }

                const deleteContact = (contact) => {
                    requestConfirm(t('confirm.delete_contact', { name: contact.name }), async () => {
                        try {
                            const res = await fetch(`/api/contacts/${contact.id}`, { method: 'DELETE' })
                            if (res.ok) {
                                if (highlightContactId.value === contact.id) highlightContactId.value = null
                                await fetchContacts()
                                await fetchTrash()
                            }
                        } catch (e) {
                            console.error('Fehler beim Löschen des Kontakts', e)
                        }
                    })
                }

                const createDepartment = async () => {
                    const name = newDeptName.value.trim()
                    if (!name) return
                    deptError.value = ''
                    try {
                        const res = await fetch('/api/departments', {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({ name })
                        })
                        if (res.ok) {
                            newDeptName.value = ''
                            await fetchDepartments()
                        } else if (res.status === 409) {
                            deptError.value = t('data.dept_duplicate')
                        } else {
                            deptError.value = t('data.dept_error')
                        }
                    } catch (e) {
                        console.error('Fehler beim Erstellen des Departments', e)
                        deptError.value = t('data.dept_error')
                    }
                }

                // --- Wiki ---
                const wikiPages = ref([])
                const wikiFilter = ref('')
                const wikiCurrent = ref(null)
                const wikiForm = ref({ id: null, title: '', content: '' })
                const wikiPreviewOpen = ref(false)
                const wikiStatus = ref('')
                const wikiStatusOk = ref(true)

                const filteredWikiPages = computed(() => {
                    const q = wikiFilter.value.trim().toLowerCase()
                    if (!q) return wikiPages.value
                    return wikiPages.value.filter(p =>
                        (p.title || '').toLowerCase().includes(q) ||
                        (p.content || '').toLowerCase().includes(q)
                    )
                })

                const fmtWikiDate = (ts) => {
                    if (!ts) return ''
                    const m = String(ts).match(/^(\d{4})-(\d{2})-(\d{2})/)
                    if (!m) return String(ts)
                    return new Date(+m[1], +m[2] - 1, +m[3]).toLocaleDateString(locale(), { year: 'numeric', month: '2-digit', day: '2-digit' })
                }

                const wikiPreviewHtml = computed(() => {
                    if (!wikiForm.value.content) return '<p class="text-zinc-600 dark:text-zinc-500">' + t('wiki.no_content') + '</p>'
                    return renderMarkdown(wikiForm.value.content, [])
                })

                const fetchWikiPages = async () => {
                    try {
                        const res = await fetch('/api/wiki')
                        if (res.ok) wikiPages.value = await res.json()
                    } catch (e) {
                        console.error('Fehler beim Laden der Wiki-Seiten', e)
                    }
                }

                const openWikiPage = (page) => {
                    wikiCurrent.value = page
                    wikiForm.value = { id: page.id, title: page.title, content: page.content || '' }
                    wikiPreviewOpen.value = !!((page.content || '').trim())
                    wikiStatus.value = ''
                    showAutocomplete.value = false
                    autocompleteResults.value = []
                }

                const newWikiPage = () => {
                    wikiCurrent.value = { id: null }
                    wikiForm.value = { id: null, title: '', content: '' }
                    wikiPreviewOpen.value = false
                    wikiStatus.value = ''
                    showAutocomplete.value = false
                    autocompleteResults.value = []
                }

                const saveWikiPage = async () => {
                    const title = wikiForm.value.title.trim()
                    if (!title) {
                        wikiStatus.value = t('wiki.missing_title')
                        wikiStatusOk.value = false
                        return
                    }
                    const isNew = !wikiForm.value.id
                    const url = isNew ? '/api/wiki' : `/api/wiki/${wikiForm.value.id}`
                    const method = isNew ? 'POST' : 'PUT'
                    try {
                        const res = await fetch(url, {
                            method,
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({ title, content: wikiForm.value.content })
                        })
                        if (res.ok) {
                            const page = await res.json()
                            wikiStatus.value = t('wiki.saved')
                            wikiStatusOk.value = true
                            await fetchWikiPages()
                            openWikiPage(page)
                        } else {
                            wikiStatus.value = t('wiki.error_saving')
                            wikiStatusOk.value = false
                        }
                    } catch (e) {
                        console.error('Fehler beim Speichern der Wiki-Seite', e)
                        wikiStatus.value = t('wiki.error_saving')
                        wikiStatusOk.value = false
                    }
                }

                const deleteWikiPage = (id) => {
                    requestConfirm(t('confirm.delete_wiki'), async () => {
                        try {
                            const res = await fetch(`/api/wiki/${id}`, { method: 'DELETE' })
                            if (res.ok) {
                                wikiCurrent.value = null
                                wikiForm.value = { id: null, title: '', content: '' }
                                wikiStatus.value = ''
                                await fetchWikiPages()
                            }
                        } catch (e) {
                            console.error('Fehler beim Löschen der Wiki-Seite', e)
                        }
                    })
                }

                const switchView = (view) => {
                    activeView.value = view
                    showAutocomplete.value = false
                    autocompleteResults.value = []
                    if (view === 'wiki' && wikiPages.value.length === 0) fetchWikiPages()
                    if (view !== 'wiki') wikiStatus.value = ''
                }

                const deleteDepartment = (name) => {
                    requestConfirm(t('confirm.delete_department', { name }), async () => {
                        try {
                            const res = await fetch('/api/departments/delete', {
                                method: 'POST',
                                headers: { 'Content-Type': 'application/json' },
                                body: JSON.stringify({ name })
                            })
                            if (res.ok) {
                                departments.value = await res.json()
                                await fetchNotes()
                                await fetchContacts()
                            }
                        } catch (e) {
                            console.error('Fehler beim Löschen des Departments', e)
                        }
                    })
                }

                // --- Dashboard computeds ---
                const dashboardActivityDays = 30
                const todayDS = () => dateStr(new Date())

                const dashboardTodayEvents = computed(() => calEventsForDate(new Date()))

                const overdueNotes = computed(() => notes.value.filter(n => isOverdue(n)))

                const dashboardUpcoming = computed(() => {
                    const today = todayDS()
                    const list = []
                    notes.value.forEach(note => {
                        if (note.status === 'archived') return
                        getAppointments(note).forEach(apt => {
                            if (apt.done || !apt.start) return
                            if (apt.start >= today) {
                                const isOverdueApt = apt.start < today
                                list.push({
                                    title: `${note.title} – ${apt.title || t('reminder.apt')}`,
                                    noteId: note.id,
                                    start: apt.start,
                                    time: apt.time || ''
                                })
                            }
                        })
                    })
                    list.sort((a, b) => (a.start + ' ' + a.time).localeCompare(b.start + ' ' + b.time))
                    return list.slice(0, 10)
                })

                const dashboardStatusRows = computed(() => {
                    const total = Math.max(notes.value.length, 1)
                    return columns.map(col => {
                        const count = notes.value.filter(n => n.status === col.id).length
                        const color = {
                            backlog: 'bg-zinc-400 dark:bg-zinc-500',
                            in_progress: 'bg-blue-500',
                            review: 'bg-amber-500',
                            done: 'bg-emerald-500',
                            archived: 'bg-zinc-300 dark:bg-zinc-700'
                        }[col.id]
                        return { id: col.id, title: statusTitle(col.id), count, pct: Math.round((count / total) * 100), color }
                    })
                })

                const dashboardPriorityRows = computed(() => {
                    const active = notes.value.filter(n => n.status !== 'done' && n.status !== 'archived')
                    const total = Math.max(active.length, 1)
                    return [
                        { label: 'high', count: active.filter(n => n.priority === 'high').length, color: 'bg-red-500' },
                        { label: 'medium', count: active.filter(n => n.priority === 'medium').length, color: 'bg-amber-500' },
                        { label: 'low', count: active.filter(n => n.priority === 'low').length, color: 'bg-zinc-400 dark:bg-zinc-500' }
                    ].map(r => ({ ...r, pct: Math.round((r.count / total) * 100) }))
                })

                const dashboardTodo = computed(() => {
                    let done = 0, total = 0
                    notes.value.forEach(note => {
                        if (note.status === 'archived') return
                        const p = getChecklistProgress(note)
                        if (p) { done += p.done; total += p.total }
                    })
                    return { done, total, pct: total ? Math.round((done / total) * 100) : 0 }
                })

                const dashboardDeptRows = computed(() => {
                    const map = {}
                    notes.value.forEach(note => {
                        if (note.status === 'archived') return
                        getDepartments(note).forEach(dep => {
                            const key = dep.toLowerCase()
                            map[key] = map[key] || { name: dep, count: 0 }
                            map[key].count++
                        })
                    })
                    const rows = Object.values(map)
                    const total = Math.max(rows.reduce((s, r) => s + r.count, 0), 1)
                    return rows
                        .sort((a, b) => b.count - a.count)
                        .map(r => ({ ...r, pct: Math.round((r.count / total) * 100) }))
                })

                const dashboardActivity = computed(() => {
                    const days = dashboardActivityDays
                    const counts = new Array(days).fill(0)
                    const labels = []
                    const today = new Date()
                    for (let i = days - 1; i >= 0; i--) {
                        const d = new Date(today)
                        d.setDate(today.getDate() - i)
                        labels.push(dateStr(d))
                    }
                    const indexOf = (s) => labels.indexOf(s)
                    notes.value.forEach(note => {
                        if (note.status !== 'done' && note.status !== 'archived') return
                        let ds = note.completed_at
                        if (!ds) ds = note.date ? convertDateToISO(note.date) : null
                        if (!ds) return
                        const idx = indexOf(ds)
                        if (idx >= 0) counts[idx]++
                    })
                    const max = Math.max(1, ...counts)
                    return counts.map((c, i) => ({
                        label: labels[i],
                        count: c,
                        height: Math.round((c / max) * 60) + (c ? 4 : 2)
                    }))
                })

                const deptContext = computed(() => {
                    if (deptDropdownSource.value === 'modal') {
                        return {
                            val: () => modalDeptInput.value,
                            input: () => modalDeptInput.value
                        }
                    }
                    if (deptDropdownSource.value === 'contact') {
                        return {
                            val: () => contactDeptInput.value,
                            input: () => contactDeptInput.value
                        }
                    }
                    return {
                        val: () => newNoteDeptInput.value,
                        input: () => newNoteDeptInput.value
                    }
                })

                const filteredDepartments = computed(() => {
                    const q = deptContext.value.val().trim().toLowerCase()
                    if (!q) return departments.value.slice(0, 8)
                    return departments.value
                        .filter(dep => dep.name.toLowerCase().includes(q))
                        .slice(0, 8)
                })

                const openDeptDropdown = (source) => {
                    deptDropdownSource.value = source
                    deptIndex.value = 0
                    deptDropdownOpen.value = true
                }

                const closeDeptDropdown = () => {
                    deptDropdownOpen.value = false
                }

                const selectDepartment = (name) => {
                    if (deptDropdownSource.value === 'modal') {
                        if (activeNoteDepartments.value.some(d => d.toLowerCase() === name.toLowerCase())) {
                            modalDeptInput.value = ''
                            closeDeptDropdown()
                            return
                        }
                        activeNoteDepartments.value.push(name)
                        modalDeptInput.value = ''
                    } else if (deptDropdownSource.value === 'contact') {
                        if (contactForm.value.departments.some(d => d.toLowerCase() === name.toLowerCase())) {
                            contactDeptInput.value = ''
                            closeDeptDropdown()
                            return
                        }
                        contactForm.value.departments.push(name)
                        contactDeptInput.value = ''
                    } else {
                        if (newNoteDepartments.value.some(d => d.toLowerCase() === name.toLowerCase())) {
                            newNoteDeptInput.value = ''
                            closeDeptDropdown()
                            return
                        }
                        newNoteDepartments.value.push(name)
                        newNoteDeptInput.value = ''
                    }
                    closeDeptDropdown()
                }

                const addCustomDepartment = (source) => {
                    let input, target
                    if (source === 'modal') {
                        input = modalDeptInput.value
                        target = activeNoteDepartments.value
                    } else if (source === 'contact') {
                        input = contactDeptInput.value
                        target = contactForm.value.departments
                    } else {
                        input = newNoteDeptInput.value
                        target = newNoteDepartments.value
                    }
                    const name = input.trim()
                    if (!name) return
                    if (!target.some(d => d.toLowerCase() === name.toLowerCase())) {
                        target.push(name)
                    }
                    if (source === 'modal') modalDeptInput.value = ''
                    else if (source === 'contact') contactDeptInput.value = ''
                    else newNoteDeptInput.value = ''
                    if (source === 'modal') {
                        if (modalDepartmentInputRef.value) modalDepartmentInputRef.value.focus()
                    } else if (source === 'contact') {
                        if (contactDepartmentInputRef.value) contactDepartmentInputRef.value.focus()
                    } else {
                        if (newNoteDepartmentInputRef.value) newNoteDepartmentInputRef.value.focus()
                    }
                }

                const removeContactDepartment = (index) => {
                    contactForm.value.departments.splice(index, 1)
                }

                const removeNewNoteDepartment = (index) => {
                    newNoteDepartments.value.splice(index, 1)
                }

                const removeActiveNoteDepartment = (index) => {
                    activeNoteDepartments.value.splice(index, 1)
                }

                const onDeptInput = () => {
                    deptIndex.value = 0
                }

                const handleDeptKeydown = (e) => {
                    const items = filteredDepartments.value
                    if (e.key === 'ArrowDown') {
                        if (!deptDropdownOpen.value) return
                        e.preventDefault()
                        if (items.length === 0) return
                        deptIndex.value = (deptIndex.value + 1) % items.length
                    } else if (e.key === 'ArrowUp') {
                        if (!deptDropdownOpen.value) return
                        e.preventDefault()
                        if (items.length === 0) return
                        deptIndex.value = (deptIndex.value - 1 + items.length) % items.length
                    } else if (e.key === 'Enter') {
                        e.preventDefault()
                        e.stopPropagation()
                        if (!deptDropdownOpen.value) {
                            addCustomDepartment(deptDropdownSource.value)
                        } else if (items.length > 0) {
                            if (deptIndex.value >= items.length) deptIndex.value = items.length - 1
                            selectDepartment(items[deptIndex.value].name)
                        } else {
                            addCustomDepartment(deptDropdownSource.value)
                        }
                    } else if (e.key === 'Escape') {
                        if (deptDropdownOpen.value) e.preventDefault()
                        e.stopPropagation()
                        closeDeptDropdown()
                    }
                }

                const focusSearchInput = () => {
                    if (searchInputRef.value) searchInputRef.value.focus()
                }

                const handleGlobalClick = (e) => {
                    const t = e.target
                    const inside = (sel) => t && t.closest ? !!t.closest(sel) : false
                    if (contextMenu.value.show && !inside('.fmt-menu')) contextMenu.value.show = false
                    if (exportMenuOpen.value && !inside('.daten-anchor')) exportMenuOpen.value = false
                    if (mobileMenuOpen.value && !inside('.burger')) mobileMenuOpen.value = false
                    if (timerOpen.value && !inside('.timer-anchor')) timerOpen.value = false
                    if (reminderOpen.value && !inside('.reminder-panel')) reminderOpen.value = false
                }

                const handleGlobalKeydown = (e) => {
                    const isCtrlOrMeta = e.ctrlKey || e.metaKey;
                    const isAlt = e.altKey;
                    const target = e.target;
                    const isTyping = target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' ||
                        target.tagName === 'SELECT' || target.isContentEditable);

                    if (isAlt && (e.key === 'n' || e.key === 'N' || e.code === 'KeyN')) {
                        e.preventDefault()
                        e.stopPropagation()
                        e.stopImmediatePropagation()
                        openNewNote()
                    } else if (!isCtrlOrMeta && !isAlt && (e.key === 'n' || e.key === 'N') && !isTyping) {
                        e.preventDefault()
                        e.stopPropagation()
                        e.stopImmediatePropagation()
                        openNewNote()
                    } else if (isCtrlOrMeta && (e.key === 'k' || e.key === 'K' || e.code === 'KeyK')) {
                        e.preventDefault()
                        e.stopPropagation()
                        e.stopImmediatePropagation()
                        focusSearchInput()
                    } else if (isCtrlOrMeta && (e.key === 's' || e.key === 'S')) {
                        if (isModalOpen.value || isNewNoteOpen.value) {
                            e.preventDefault()
                            e.stopPropagation()
                            e.stopImmediatePropagation()
                            if (isModalOpen.value) saveActiveNote()
                            else createNote()
                        }
                    } else if (e.key === '?' && !isTyping) {
                        e.preventDefault()
                        e.stopPropagation()
                        e.stopImmediatePropagation()
                        shortcutHelpOpen.value = !shortcutHelpOpen.value
                    } else if (e.key === 'Escape') {
                        e.preventDefault()
                        e.stopPropagation()
                        e.stopImmediatePropagation()
                        if (showAutocomplete.value) {
                            cancelAutocomplete()
                        } else if (confirmOpen.value) {
                            confirmCancel()
                        } else if (contextMenu.value.show) {
                            closeContextMenu()
                        } else if (deptDropdownOpen.value) {
                            closeDeptDropdown()
                        } else if (contactModalOpen.value) {
                            contactModalOpen.value = false
                        } else if (isSettingsOpen.value) {
                            isSettingsOpen.value = false
                        } else if (exportMenuOpen.value) {
                            exportMenuOpen.value = false
                        } else if (mobileMenuOpen.value) {
                            mobileMenuOpen.value = false
                        } else if (timerOpen.value) {
                            timerOpen.value = false
                        } else if (shortcutHelpOpen.value) {
                            shortcutHelpOpen.value = false
                        } else if (reminderOpen.value) {
                            reminderOpen.value = false
                        } else if (isModalOpen.value) {
                            closeModal()
                        } else if (isNewNoteOpen.value) {
                            closeNewNote()
                        }
                    }
                }

                onMounted(() => {
                    document.documentElement.classList.toggle('dark', isDark.value)
                    fetchNotes()
                    fetchDepartments()
                    fetchContacts()
                    fetchTrash()
                    fetchArchivedNotes()
fetchWikiPages()
                    if (activeView.value === 'graph') {
                        rebuildGraph()
                        graphWarm()
                        graphStart()
                    }
                    window.addEventListener('keydown', handleGlobalKeydown, { capture: true })
                    document.addEventListener('click', handleGlobalClick)
                    startReminderLoop()
                })

                onUnmounted(() => {
                    window.removeEventListener('keydown', handleGlobalKeydown, { capture: true })
                    document.removeEventListener('click', handleGlobalClick)
                    clearInterval(reminderInterval)
                    clearInterval(timerInterval)
                })

                const dueStatus = (note) => {
                    if (note.status === 'done' || note.status === 'archived' || !note.due_date) return 'none'
                    const now = new Date()
                    const todayStr = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`
                    if (note.due_date < todayStr) return 'overdue'
                    if (note.due_date === todayStr) return 'due'
                    const due = new Date(note.due_date + 'T00:00:00')
                    const today = new Date(todayStr + 'T00:00:00')
                    if (Math.round((due - today) / 86400000) <= 3) return 'due'
                    return 'none'
                }

                const isOverdue = (note) => dueStatus(note) === 'overdue'
                const isDueSoon = (note) => dueStatus(note) === 'due'

                // --- Draft-Autosave for the new-note modal (localStorage) ---
                const fmtClock = (ts) => {
                    const d = new Date(ts)
                    return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
                }
                let draftTimer = null
                const hasNewNoteDraftContent = () =>
                    newNoteTitle.value.trim() || newNoteContent.value.trim() ||
                    newNoteChecklist.value.length || newNoteAppointments.value.length
                const persistNewNoteDraft = () => {
                    if (!isNewNoteOpen.value || !hasNewNoteDraftContent()) return
                    localStorage['notice-draft'] = JSON.stringify({
                        title: newNoteTitle.value,
                        priority: newNotePriority.value,
                        due_date: newNoteDueDate.value,
                        repeat_rule: newNoteRepeatRule.value,
                        status: newNoteStatus.value,
                        content: newNoteContent.value,
                        checklist: newNoteChecklist.value,
                        departments: newNoteDepartments.value,
                        appointments: newNoteAppointments.value,
                        ts: Date.now()
                    })
                    draftTs.value = Date.now()
                    draftRestored.value = false
                }
                const scheduleDraftPersist = () => {
                    clearTimeout(draftTimer)
                    if (!isNewNoteOpen.value) return
                    draftTimer = setTimeout(() => persistNewNoteDraft(), 800)
                }
                const clearNewNoteDraft = () => {
                    clearTimeout(draftTimer)
                    localStorage.removeItem('notice-draft')
                    draftTs.value = null
                    draftRestored.value = false
                }
                watch([newNoteTitle, newNoteContent, newNoteChecklist, newNotePriority,
                       newNoteDueDate, newNoteRepeatRule, newNoteStatus,
                       newNoteDepartments, newNoteAppointments],
                      scheduleDraftPersist, { deep: true })

                const openNewNote = () => {
                    isNewNoteOpen.value = true
                    newNoteMarkdownMax.value = false
                    try {
                        const d = JSON.parse(localStorage['notice-draft'] || 'null')
                        if (d && d.ts && (d.title || d.content || (d.checklist || []).length || (d.appointments || []).length)) {
                            newNoteTitle.value = d.title || ''
                            newNoteContent.value = d.content || ''
                            newNoteChecklist.value = d.checklist || []
                            newNoteDepartments.value = d.departments || []
                            newNoteAppointments.value = d.appointments || []
                            newNotePriority.value = d.priority || 'medium'
                            newNoteDueDate.value = d.due_date || ''
                            newNoteRepeatRule.value = d.repeat_rule || ''
                            newNoteStatus.value = d.status || 'backlog'
                            draftTs.value = d.ts
                            draftRestored.value = true
                        }
                    } catch (e) { }
                    requestAnimationFrame(() => {
                        if (newNoteTitleInputRef.value) {
                            newNoteTitleInputRef.value.focus()
                        }
                    })
                }

                const closeNewNote = () => {
                    if (draftTs.value) {
                        if (hasNewNoteDraftContent()) persistNewNoteDraft()
                        else clearNewNoteDraft()
                    }
                    isNewNoteOpen.value = false
                    newNoteTitle.value = ''
                    newNotePriority.value = 'medium'
                    newNoteDueDate.value = ''
                    newNoteRepeatRule.value = ''
                    newNoteStatus.value = 'backlog'
                    newNoteContent.value = ''
                    newNoteChecklist.value = []
                    newNoteStepText.value = ''
                    newNoteDepartment.value = ''
                    newNoteDepartments.value = []
                    newNoteDeptInput.value = ''
                    newNoteAppointments.value = []
                    newAptTitle.value = ''
                    newAptStart.value = ''
                    newAptEnd.value = ''
                    newAptTime.value = ''
                    newAptEndTime.value = ''
                    newAptHasEnd.value = false
                    closeDeptDropdown()
                }

                const addNewNoteStep = () => {
                    if (!newNoteStepText.value.trim()) return
                    newNoteChecklist.value.push({ text: newNoteStepText.value.trim(), done: false })
                    newNoteStepText.value = ''
                }

                const removeNewNoteStep = (index) => {
                    newNoteChecklist.value.splice(index, 1)
                }

                const addNewAppointment = () => {
                    if (!newAptTitle.value.trim() || !newAptStart.value) return
                    const apt = { title: newAptTitle.value.trim(), start: newAptStart.value }
                    if (newAptTime.value) apt.time = newAptTime.value
                    if (newAptHasEnd.value && newAptEnd.value && newAptEnd.value >= newAptStart.value) apt.end = newAptEnd.value
                    if (apt.end && newAptEndTime.value) apt.endTime = newAptEndTime.value
                    newNoteAppointments.value.push(apt)
                    newAptTitle.value = ''
                    newAptStart.value = ''
                    newAptEnd.value = ''
                    newAptTime.value = ''
                    newAptEndTime.value = ''
                    newAptHasEnd.value = false
                }

                const addModalAppointment = () => {
                    if (!modalAptTitle.value.trim() || !modalAptStart.value) return
                    const apt = { title: modalAptTitle.value.trim(), start: modalAptStart.value }
                    if (modalAptTime.value) apt.time = modalAptTime.value
                    if (modalAptHasEnd.value && modalAptEnd.value && modalAptEnd.value >= modalAptStart.value) apt.end = modalAptEnd.value
                    if (apt.end && modalAptEndTime.value) apt.endTime = modalAptEndTime.value
                    activeNoteAppointments.value.push(apt)
                    modalAptTitle.value = ''
                    modalAptStart.value = ''
                    modalAptEnd.value = ''
                    modalAptTime.value = ''
                    modalAptEndTime.value = ''
                    modalAptHasEnd.value = false
                }

                const handleNewNoteEnter = (e) => {
                    if (deptDropdownOpen.value) return
                    if (e.target && (e.target.tagName === 'SELECT' || e.target.tagName === 'BUTTON')) return
                    if (newNoteStepInputRef.value && e.target === newNoteStepInputRef.value) return
                    if (newNoteDepartmentInputRef.value && e.target === newNoteDepartmentInputRef.value) return
                    if (newNoteTextareaRef.value && e.target === newNoteTextareaRef.value) return
                    e.preventDefault()
                    createNote()
                }

                const createNote = async () => {
                    if (!newNoteTitle.value.trim()) return

                    const storagePayload = {
                        text: newNoteContent.value,
                        checklist: newNoteChecklist.value
                    }

                    try {
                        const res = await fetch('/api/notes', {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({
                                title: newNoteTitle.value.trim(),
                                content: JSON.stringify(storagePayload),
                                status: newNoteStatus.value,
                                priority: newNotePriority.value,
                                due_date: newNoteDueDate.value || null,
                                departments: JSON.stringify(newNoteDepartments.value),
                                appointments: JSON.stringify(newNoteAppointments.value),
                                repeat_rule: newNoteRepeatRule.value || null
                            })
                        })
                        if (res.ok) {
                            const createdNote = await res.json()
                            notes.value.push(createdNote)
                            fetchDepartments()
                            pushUndo(t('undo.created', { title: createdNote.title }), async () => {
                                const id = createdNote.id
                                notes.value = notes.value.filter(n => n.id !== id)
                                await fetch(`/api/notes/${id}/force`, { method: 'DELETE' })
                            })
                            clearNewNoteDraft()
                            closeNewNote()
                        }
                    } catch (e) {
                        console.error('Fehler beim Erstellen', e)
                    }
                }

                const downloadText = (filename, content, mime) => {
                    if (window.NoticeBridge && typeof window.NoticeBridge.saveFile === 'function') {
                        window.NoticeBridge.saveFile(filename, content, mime || 'text/plain')
                        return
                    }
                    const dataStr = "data:" + (mime || 'text/plain') + ";charset=utf-8," + encodeURIComponent(content)
                    const downloadAnchor = document.createElement('a')
                    downloadAnchor.setAttribute("href", dataStr)
                    downloadAnchor.setAttribute("download", filename)
                    document.body.appendChild(downloadAnchor)
                    downloadAnchor.click()
                    downloadAnchor.remove()
                }

                const exportJson = () => {
                    if (notes.value.length === 0) return
                    downloadText('kanban_notes_export.json', JSON.stringify(notes.value, null, 2), 'application/json')
                }

                const exportCsv = () => {
                    if (notes.value.length === 0) return
                    let csvContent = "ID,Title,Status,Priority,Date,DueDate,Departments,Appointments\r\n";
                    notes.value.forEach(note => {
                        const depts = getDepartments(note).join('; ')
                        const appts = getAppointments(note).map(a => `${a.title} (${a.start})`).join('; ')
                        let row = [note.id, `"${note.title.replace(/"/g, '""')}"`, note.status, note.priority, note.date, note.due_date || '', `"${depts.replace(/"/g, '""')}"`, `"${appts.replace(/"/g, '""')}"`];
                        csvContent += row.join(",") + "\r\n";
                    });
                    downloadText('kanban_notes_export.csv', csvContent, 'text/csv')
                }

                const exportFullBackup = () => {
                    const data = {
                        app: 'notice',
                        version: '1.7',
                        exported: new Date().toISOString(),
                        notes: notes.value,
                        contacts: contacts.value,
                        wikiPages: wikiPages.value
                    }
                    downloadText(`notice_backup_${new Date().toISOString().slice(0, 10)}.json`,
                        JSON.stringify(data, null, 2), 'application/json')
                }

                const startImport = (mode) => {
                    importMode.value = mode
                    if (importFileInputRef.value) importFileInputRef.value.click()
                }

                const onImportFile = async (e) => {
                    const file = e.target.files && e.target.files[0]
                    e.target.value = ''
                    if (!file) return
                    let data
                    try {
                        data = JSON.parse(await file.text())
                    } catch {
                        alert(t('import.invalid_json'))
                        return
                    }
                    if (!data || typeof data !== 'object') {
                        alert(t('import.parsing_failed'))
                        return
                    }
                    const pnotes = Array.isArray(data) ? data : (Array.isArray(data.notes) ? data.notes : null)
                    const pcontacts = (!Array.isArray(data) && Array.isArray(data.contacts)) ? data.contacts : []
                    const pwiki = (!Array.isArray(data) && Array.isArray(data.wikiPages)) ? data.wikiPages : []
                    if (!pnotes) {
                        alert(t('import.no_notes'))
                        return
                    }
                    existingCount.value = notes.value.length
                    existingContactCount.value = contacts.value.length
                    pendingImport.value = { fileName: file.name, mode: importMode.value, notes: pnotes, contacts: pcontacts, wiki: pwiki }
                }

                const confirmImport = async () => {
                    const imp = pendingImport.value
                    if (!imp) return
                    pendingImport.value = null
                    try {
                        const res = await fetch('/api/import', {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({ mode: imp.mode, notes: imp.notes, contacts: imp.contacts, wiki: imp.wiki })
                        })
                        if (res.ok) {
                            const r = await res.json()
                            alert(t('import.done', { notes: r.notes_imported, skipped: r.notes_skipped, contacts: r.contacts_imported, contact_skipped: r.contacts_skipped }) + (r.wiki_imported ? ' / ' + r.wiki_imported + ' Wiki' : ''))
                            await fetchNotes()
                            await fetchContacts()
                            await fetchDepartments()
                            await fetchTrash()
                            await fetchWikiPages()
                        } else {
                            alert(t('import.failed'))
                        }
                    } catch (err) {
                        alert(t('import.failed'))
                    }
                }

                // --- Papierkorb (soft-deleted notes & contacts) ---
                const fetchTrash = async () => {
                    try {
                        const res = await fetch('/api/trash')
                        if (res.ok) {
                            const data = await res.json()
                            trashNotes.value = data.notes || []
                            trashContacts.value = data.contacts || []
                        }
                    } catch (e) {
                        console.error('Fehler beim Laden des Papierkorbs', e)
                    }
                }

                const restoreNote = async (id) => {
                    const res = await fetch(`/api/notes/${id}/restore`, { method: 'POST' })
                    if (res.ok) {
                        await fetchTrash()
                        await fetchNotes()
                    }
                }

                const restoreContact = async (id) => {
                    const res = await fetch(`/api/contacts/${id}/restore`, { method: 'POST' })
                    if (res.ok) {
                        await fetchTrash()
                        await fetchContacts()
                    }
                }

                const forceDeleteNote = (id) => {
                    requestConfirm(t('confirm.delete_note_force'), async () => {
                        const res = await fetch(`/api/notes/${id}/force`, { method: 'DELETE' })
                        if (res.ok) await fetchTrash()
                    })
                }

                const forceDeleteContact = (id) => {
                    requestConfirm(t('confirm.delete_contact_force'), async () => {
                        const res = await fetch(`/api/contacts/${id}/force`, { method: 'DELETE' })
                        if (res.ok) await fetchTrash()
                    })
                }

                const clearTrash = () => {
                    requestConfirm(t('confirm.empty_trash'), async () => {
                        const res = await fetch('/api/trash/clear', { method: 'POST' })
                        if (res.ok) await fetchTrash()
                    })
                }

                const openModal = (note) => {
                    activeNote.value = { ...note }
                    try {
                        const parsed = JSON.parse(note.content);
                        if (parsed && typeof parsed === 'object' && parsed.text !== undefined) {
                            activeNote.value.content = parsed.text || '';
                            activeChecklist.value = parsed.checklist || [];
                        } else {
                            activeChecklist.value = [];
                        }
                    } catch {
                        activeChecklist.value = [];
                    }
                    activeNoteDepartments.value = getDepartments(note)
                    activeNoteAppointments.value = getAppointments(note)
                    modalDeptInput.value = ''
                    isPreviewMode.value = true
                    detailMarkdownMax.value = false
                    isModalOpen.value = true
                    closeContextMenu()
                }

                const closeModal = () => {
                    isModalOpen.value = false
                    activeNote.value = null
                    activeChecklist.value = []
                    newStepText.value = ''
                    activeNoteAppointments.value = []
                    activeNoteDepartments.value = []
                    modalDeptInput.value = ''
                    closeContextMenu()
                    showAutocomplete.value = false
                    clearTimeout(autocompleteTimer)
                    collectOverdueReminders()
                }

                // Context menu logic
                const openContextMenu = (e) => {
                    const ta = e.target
                    if (!ta || ta.tagName !== 'TEXTAREA') return
                    const cx = e.clientX
                    const cy = e.clientY
                    contextMenu.value = {
                        show: true,
                        x: cx,
                        y: cy,
                        source: ta === newNoteTextareaRef.value ? 'new' : (ta === wikiTextareaRef.value ? 'wiki' : 'edit'),
                        selectionStart: ta.selectionStart,
                        selectionEnd: ta.selectionEnd
                    }
                    nextTick(() => {
                        const menu = document.querySelector('.fmt-menu')
                        if (!menu) return
                        const mr = menu.getBoundingClientRect()
                        const pad = 8
                        if (mr.bottom > window.innerHeight - pad) {
                            contextMenu.value.y = Math.max(pad, cy - mr.height - pad)
                        }
                        if (mr.right > window.innerWidth - pad) {
                            contextMenu.value.x = Math.max(pad, cx - mr.width - pad)
                        }
                    })
                }

                const closeContextMenu = () => {
                    contextMenu.value.show = false
                    exportMenuOpen.value = false
                }

                // --- Autocomplete for [[ note links ---
                const searchAutocomplete = async (q) => {
                    try {
                        const url = autocompleteType.value === 'address'
                            ? `/api/contacts/search?q=${encodeURIComponent(q)}`
                            : (autocompleteType.value === 'wiki'
                                ? `/api/wiki/search?q=${encodeURIComponent(q)}`
                                : `/api/notes/search?q=${encodeURIComponent(q)}`)
                        const res = await fetch(url)
                        if (res.ok) {
                            let data = await res.json()
                            if (autocompleteType.value === 'address') {
                                data = data.map(c => ({ id: c.id, title: c.name }))
                            } else if (autocompleteType.value === 'wiki') {
                                data = data.map(p => ({ id: p.id, title: p.name }))
                            }
                            autocompleteResults.value = data
                            autocompleteIndex.value = 0
                            showAutocomplete.value = true
                            updateAutocompletePos()
                        } else {
                            showAutocomplete.value = false
                        }
                    } catch (e) {
                        showAutocomplete.value = false
                    }
                }

                const inWikiEditor = () => activeView.value === 'wiki' && !wikiPreviewOpen.value && !!wikiTextareaRef.value
                const acTextarea = () => {
                    if (isNewNoteOpen.value) return newNoteTextareaRef.value
                    if (isModalOpen.value) return textareaRef.value
                    if (inWikiEditor()) return wikiTextareaRef.value
                    return null
                }
                const acMirror = () => {
                    if (isNewNoteOpen.value) return newNoteCaretMirrorRef.value
                    if (isModalOpen.value) return caretMirrorRef.value
                    if (inWikiEditor()) return wikiCaretMirrorRef.value
                    return null
                }
                const acGetContent = () => {
                    if (isNewNoteOpen.value) return (newNoteContent.value || '')
                    if (isModalOpen.value) return (activeNote.value ? activeNote.value.content : '')
                    if (inWikiEditor()) return (wikiForm.value.content || '')
                    return ''
                }
                const acTextareaValue = () => {
                    const ta = acTextarea()
                    return ta ? ta.value : acGetContent()
                }
                const acSetContent = (str) => {
                    if (isNewNoteOpen.value) newNoteContent.value = str
                    else if (isModalOpen.value) { if (activeNote.value) activeNote.value.content = str }
                    else if (inWikiEditor()) wikiForm.value.content = str
                }

                const updateAutocompletePos = () => {
                    const ta = acTextarea()
                    const mirror = acMirror()
                    if (!ta || !mirror) return
                    const cursor = ta.selectionStart
                    const text = acTextareaValue()
                    const c = Math.min(cursor, text.length)
                    const before = text.substring(0, c)
                    const line = before.split('\n').length - 1
                    const colLines = before.split('\n')
                    const col = colLines[colLines.length - 1].length

                    // Update the caret mirror: text with a marking span at the cursor position
                    let html = ''
                    for (let i = 0; i < colLines.length; i++) {
                        const chunk = colLines[i]
                        if (i < colLines.length - 1) {
                            html += chunk.replace(/</g, '&lt;') + '\n'
                        } else {
                            // last line
                            const colonIdx = chunk.lastIndexOf(':')
                            if (colonIdx >= col) {
                                // cursor on this line, place marker after the prefix
                                html += chunk.substring(0, col).replace(/</g, '&lt;')
                                html += '<span id="caret-marker">a</span>'
                            } else {
                                html += chunk.substring(0, col).replace(/</g, '&lt;')
                                html += '<span id="caret-marker">a</span>'
                            }
                        }
                    }
                    mirror.innerHTML = html + '<br>'

                    const marker = mirror.querySelector('#caret-marker')
                    if (!marker) return
                    const taRect = ta.getBoundingClientRect()
                    const markRect = marker.getBoundingClientRect()
                    // Offset result to the dropdown's containing block (nearest positioned ancestor)
                    let cont = ta.parentElement
                    while (cont && getComputedStyle(cont).position === 'static') cont = cont.parentElement
                    const contRect = (cont || document.body).getBoundingClientRect()
                    const x = (markRect.left - taRect.left) + (taRect.left - contRect.left)
                    const y = (markRect.top - taRect.top) + (taRect.top - contRect.top)

                    // Flip: if there is enough space above / not below, show the dropdown upward
                    const container = ta.closest('.md\\:col-span-2') || ta.parentElement
                    const containerHeight = container ? container.clientHeight : 300
                    const estHeight = Math.min(224, (autocompleteResults.value.length * 26) + 30)
                    const flip = (y + 24 + estHeight) > containerHeight && y > estHeight
                    autocompletePos.value = { x, y: flip ? (y - estHeight) : (y + 24), flip }
                }

                const cancelAutocomplete = () => {
                    clearTimeout(autocompleteTimer)
                    const ta = acTextarea()
                    if (ta) {
                        const text = acTextareaValue()
                        const cursor = Math.min(ta.selectionStart, text.length)
                        const start = Math.min(autocompleteStart.value, cursor)
                        const removeFrom = Math.max(0, start - 2)
                        const before = text.substring(removeFrom, start)
                        if (before === '[[' || before === '{{' || before === '<<') {
                            acSetContent(text.substring(0, removeFrom) + text.substring(cursor))
                            const target = removeFrom
                            requestAnimationFrame(() => {
                                if (ta) {
                                    ta.selectionStart = ta.selectionEnd = target
                                    ta.focus()
                                }
                            })
                        }
                    }
                    showAutocomplete.value = false
                    autocompleteResults.value = []
                }

                const handleAutocomplete = () => {
                    const ta = acTextarea()
                    if (!ta) return
                    // Prefer the DOM value (source of truth) since v-model may not be in sync yet.
                    const text = acTextareaValue()
                    const cursor = ta.selectionStart
                    const effectiveCursor = Math.min(cursor, text.length)

                    // Detect the nearest active opener ([[, {{ or <<) before the cursor
                    const openNote = text.lastIndexOf('[[', effectiveCursor)
                    const openAddr = text.lastIndexOf('{{', effectiveCursor)
                    const openWiki = text.lastIndexOf('<<', effectiveCursor)
                    let opener = -1
                    let kind = null
                    const candidates = [[openNote, 'note'], [openAddr, 'address'], [openWiki, 'wiki']]
                    for (const [idx, k] of candidates) {
                        if (idx >= 0 && idx > opener) {
                            opener = idx
                            kind = k
                        }
                    }
                    if (opener === -1 || !kind) {
                        showAutocomplete.value = false
                        autocompleteType.value = 'note'
                        return
                    }
                    const between = text.substring(opener + 2, effectiveCursor)
                    // Close if the closer already lies between the opener and cursor
                    const endMarker = kind === 'note' ? ']]' : (kind === 'address' ? '}}' : '>>')
                    if (between.includes(endMarker)) {
                        showAutocomplete.value = false
                        autocompleteType.value = 'note'
                        return
                    }
                    // Stop after "|" (display alias is being typed)
                    if (between.includes('|')) {
                        showAutocomplete.value = false
                        autocompleteType.value = 'note'
                        return
                    }

                    autocompleteType.value = kind
                    autocompleteStart.value = opener + 2
                    autocompleteQuery.value = between
                    updateAutocompletePos()
                    clearTimeout(autocompleteTimer)
                    autocompleteTimer = setTimeout(() => {
                        searchAutocomplete(between)
                    }, 200)
                }

                const selectAutocomplete = (index) => {
                    const ta = acTextarea()
                    if (!ta) return
                    const selected = autocompleteResults.value[index]
                    if (!selected) return
                    const closer = autocompleteType.value === 'address' ? '}}' : (autocompleteType.value === 'wiki' ? '>>' : ']]')
                    const text = acTextareaValue()
                    const cursor = Math.min(ta.selectionStart, text.length)
                    const start = Math.min(autocompleteStart.value, cursor)
                    // Replace opener+partial with opener+Full title+closer
                    const newText = text.substring(0, start) + selected.title + closer + text.substring(cursor)
                    acSetContent(newText)
                    showAutocomplete.value = false
                    // Move the cursor after the closer
                    requestAnimationFrame(() => {
                        if (ta) {
                            const newCursor = start + selected.title.length + 2
                            ta.selectionStart = ta.selectionEnd = newCursor
                            ta.focus()
                        }
                    })
                }


                const handleAutocompleteKeydown = (e) => {
                    if (!showAutocomplete.value) return
                    if (e.key === 'ArrowDown') {
                        e.preventDefault()
                        autocompleteIndex.value = (autocompleteIndex.value + 1) % autocompleteResults.value.length
                    } else if (e.key === 'ArrowUp') {
                        e.preventDefault()
                        autocompleteIndex.value = (autocompleteIndex.value - 1 + autocompleteResults.value.length) % autocompleteResults.value.length
                    } else if (e.key === 'Enter') {
                        e.preventDefault()
                        selectAutocomplete(autocompleteIndex.value)
                    }
                }

                // --- Note links in preview: [[Title|Display-Name]] ---
                const processNoteLinks = (html) => {
                    return html.replace(/\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/g, (match, title, display) => {
                        const safeTitle = title.replace(/"/g, '&quot;')
                        const safeDisplay = (display || title).replace(/"/g, '&quot;')
                        return `<a class="note-link" data-note-title="${safeTitle}">${safeDisplay}</a>`
                    })
                }

                const handleNoteLinkClick = (e) => {
                    const addrLink = e.target.closest('.address-link')
                    if (addrLink) {
                        handleAddressLinkClick(e)
                        return
                    }
                    const wikiLink = e.target.closest('.wiki-link')
                    if (wikiLink) {
                        const title = wikiLink.getAttribute('data-wiki-title')
                        const page = wikiPages.value.find(p => p.title === title)
                        if (page) {
                            if (isModalOpen.value) closeModal()
                            switchView('wiki')
                            openWikiPage(page)
                        } else {
                            alert(t('wiki.not_found', { title }))
                        }
                        return
                    }
                    const link = e.target.closest('.note-link')
                    if (!link) return
                    e.preventDefault()
                    const title = link.getAttribute('data-note-title')
                    const note = notes.value.find(n => n.title === title)
                    if (note) {
                        openModal(note)
                    } else {
                        alert(t('note.not_found', { title }))
                    }
                }

                // --- Address links in preview: {{Name|display}} ---
                const processAddressLinks = (html) => {
                    return html.replace(/\{\{([^}|]+)(?:\|([^}]+))?\}\}/g, (match, name, display) => {
                        const safeName = name.replace(/"/g, '&quot;')
                        const safeDisplay = (display || name).replace(/"/g, '&quot;')
                        return `<a class="note-link address-link" data-address-name="${safeName}">${safeDisplay}</a>`
                    })
                }

                // --- Wiki links in preview: <<Page-Title|Display-Name>> ---
                const processWikiLinks = (html) => {
                    return html
                        .replace(/&lt;/g, '<')
                        .replace(/&gt;/g, '>')
                        .replace(/<<([^>|]+)(?:\|([^>]+))?>>/g, (match, title, display) => {
                            const safeTitle = title.replace(/"/g, '&quot;')
                            const safeDisplay = (display || title).replace(/"/g, '&quot;')
                            return `<a class="note-link wiki-link" data-wiki-title="${safeTitle}">${safeDisplay}</a>`
                        })
                }

                const hideContactPopover = () => {
                    contactPopover.value.show = false
                    contactPopover.value.contact = null
                }

                const showContactPopover = (e) => {
                    const link = e.target.closest('.address-link')
                    if (!link) {
                        hideContactPopover()
                        return
                    }
                    const name = link.getAttribute('data-address-name')
                    const contact = contacts.value.find(c => (c.name || '') === name)
                    const rect = link.getBoundingClientRect()
                    let cont = link
                    while (cont && getComputedStyle(cont).position === 'static') cont = cont.parentElement
                    const contRect = (cont || document.body).getBoundingClientRect()
                    contactPopover.value = {
                        show: true,
                        x: Math.min(rect.right + 6 - contRect.left, 280),
                        y: rect.bottom + 4 - contRect.top,
                        contact: contact || { name, department: null, phone: null, email: null, description: null }
                    }
                }

                const handleAddressLinkClick = (e) => {
                    const link = e.target.closest('.address-link')
                    if (!link) return
                    e.preventDefault()
                    hideContactPopover()
                    if (isModalOpen.value) closeModal()
                    const name = link.getAttribute('data-address-name')
                    const contact = contacts.value.find(c => (c.name || '') === name)
                    if (contact) {
                        highlightContactId.value = contact.id
                        activeView.value = 'data'
                    } else {
                        highlightContactId.value = null
                        activeView.value = 'data'
                        contactFilter.value = name
                    }
                }

                // --- Duplicate note ---
                const duplicateNote = async (note) => {
                    try {
                        const res = await fetch(`/api/notes/${note.id}/duplicate`, { method: 'POST' })
                        if (res.ok) {
                            const newNote = await res.json()
                            notes.value.push(newNote)
                            fetchDepartments()
                            pushUndo(t('undo.duplicated'), async () => {
                                const id = newNote.id
                                notes.value = notes.value.filter(n => n.id !== id)
                                await fetch(`/api/notes/${id}/force`, { method: 'DELETE' })
                            })
                        }
                    } catch (e) {
                        console.error('Fehler beim Duplizieren', e)
                    }
                }

                // --- Archive / restore ---
                const archiveNote = async (note) => {
                    const snapshot = { ...note }
                    try {
                        await fetch(`/api/notes/${note.id}/archive`, { method: 'POST' })
                        notes.value = notes.value.filter(n => n.id !== note.id)
                        await fetchArchivedNotes()
                        pushUndo(t('undo.archived', { title: note.title }), async () => {
                            try {
                                const archived = archivedNotes.value.find(a => a.title === snapshot.title && a.status === 'archived')
                                if (archived) await restoreFromArchive(archived.id)
                            } catch (e) {
                                console.error('Fehler beim Rückgängig-machen (Archiv)', e)
                            }
                        })
                    } catch (e) {
                        console.error('Fehler beim Archivieren', e)
                    }
                }

                const unarchiveNote = async (note) => {
                    await restoreFromArchive(note.id)
                }

                const updateNoteStatus = async (note, newStatus) => {
                    try {
                        const res = await fetch(`/api/notes/${note.id}`, {
                            method: 'PUT',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({
                                title: note.title,
                                content: note.content,
                                status: newStatus,
                                priority: note.priority,
                                due_date: note.due_date,
                                sort_order: -1
                            })
                        })
                        if (res.ok) {
                            note.status = newStatus
                            note.sort_order = -1
                        }
                    } catch (e) {
                        console.error('Fehler beim Status-Update', e)
                    }
                }

                // --- Auto-archive (settings) ---
                const autoArchiveDone = async () => {
                    if (!autoArchiveEnabled.value) return
                    const now = new Date()
                    const yr = now.getFullYear(), mo = now.getMonth()
                    const daysInMonth = new Date(yr, mo + 1, 0).getDate()
                    let targetDay = Math.min(autoArchiveDay.value || 1, daysInMonth)
                    let target = new Date(yr, mo, targetDay)
                    const dow = target.getDay()
                    if (dow === 0) target.setDate(target.getDate() + 1)
                    else if (dow === 6) target.setDate(target.getDate() + 2)
                    if (target.getMonth() !== mo) {
                        target = new Date(yr, mo, daysInMonth)
                        while (target.getDay() === 0 || target.getDay() === 6) target.setDate(target.getDate() - 1)
                    }
                    const todayStr = now.getFullYear() + '-' + String(now.getMonth() + 1).padStart(2, '0') + '-' + String(now.getDate()).padStart(2, '0')
                    const targetStr = target.getFullYear() + '-' + String(target.getMonth() + 1).padStart(2, '0') + '-' + String(target.getDate()).padStart(2, '0')
                    if (todayStr !== targetStr) return
                    const markerKey = 'notice-auto-archive-' + yr + '-' + String(mo + 1).padStart(2, '0')
                    if (localStorage.getItem(markerKey)) return
                    const candidates = notes.value.filter(n => n.status === 'done')
                    if (candidates.length === 0) return
                    for (const note of candidates) await archiveNote(note)
                    localStorage.setItem(markerKey, '1')
                }

                // --- Overdue appointment reminder (Feature 3) ---
                const reminderOpen = ref(false)
                const overdueReminders = ref([])
const isAppointmentOverdue = (apt) => {
                     if (!apt || apt.done) return false
                     if (!apt.start) return false
                    const todayStr = dateStr(new Date())
                    const nowH = new Date()
                    const nowMin = nowH.getHours() * 60 + nowH.getMinutes()
                    if (apt.end) {
                        if (apt.end < todayStr) return true
                        if (apt.end > todayStr) return false
                        if (apt.endTime) {
                            const m = apt.endTime.match(/^(\d{1,2}):(\d{2})/)
                            if (m) return nowMin > (parseInt(m[1], 10) * 60 + parseInt(m[2], 10))
                        }
                        return false
                    }
                    if (apt.start < todayStr) return true
                    if (apt.start > todayStr) return false
                    if (apt.time) {
                        const m = apt.time.match(/^(\d{1,2}):(\d{2})/)
                        if (m) return nowMin > (parseInt(m[1], 10) * 60 + parseInt(m[2], 10))
                    }
                    return false
                }

                // --- Browser notifications for overdue items ---
                const notify = (title, body) => {
                    if (typeof Notification !== 'undefined' && Notification.permission === 'granted') {
                        try { new Notification(title, { body }) } catch (e) {}
                    }
                }

                let notifiedKeys = new Set()
                try {
                    const saved = JSON.parse(localStorage.getItem('notice-notified') || '[]')
                    if (Array.isArray(saved)) notifiedKeys = new Set(saved)
                } catch (e) {}

                const collectOverdueReminders = (opts = {}) => {
                    const list = []
                    notes.value.forEach(note => {
                        if (note.status === 'archived' || note.status === 'done') return
                        const appts = getAppointments(note)
                        appts.forEach((apt, ai) => {
                            if (isAppointmentOverdue(apt)) {
                                const ev = {
                                    title: `${note.title} – ${apt.title || t('reminder.apt')}`,
                                    time: apt.time || '',
                                    start: apt.start,
                                    end: apt.end || '',
                                    endTime: apt.endTime || ''
                                }
                                list.push({
                                    note,
                                    apt,
                                    aptIdx: ai,
                                    kind: 'apt',
                                    range: fmtAptRange(ev)
                                })
                            }
                        })
                        if (isOverdue(note)) {
                            list.push({
                                note,
                                apt: { title: t('reminder.due'), start: note.due_date },
                                kind: 'due',
                                range: t('reminder.due') + ' ' + fmtDate(note.due_date)
                            })
                        }
                    })
                    const currentKeys = new Set()
                    list.forEach(r => {
                        const key = (r.kind === 'apt' ? 'apt' : 'due') + '_' + r.note.id + '_' + r.apt.start
                        currentKeys.add(key)
                        if (!notifiedKeys.has(key) && !opts.skipNotify) {
                            notifiedKeys.add(key)
                            notify(t('reminder.title'), `${r.range} – ${r.note.title}`)
                        }
                    })
                    notifiedKeys = new Set([...notifiedKeys].filter(k => currentKeys.has(k)))
                    try { localStorage.setItem('notice-notified', JSON.stringify([...notifiedKeys])) } catch (e) {}
                    overdueReminders.value = list
                    reminderOpen.value = list.length > 0
                }

                watch(overdueReminders, (list) => {
                    document.title = list.length > 0 ? `(${list.length}) Notice – Pro Kanban Notes` : 'Notice – Pro Kanban Notes'
                })

                const openNoteFromReminder = (ri) => {
                    const r = overdueReminders.value[ri]
                    if (!r) return
                    openModal(r.note)
                    overdueReminders.value.splice(ri, 1)
                    if (overdueReminders.value.length === 0) reminderOpen.value = false
                }

                const markApptDone = async (note, aptIndex, done) => {
                    if (!note) return
                    const appts = getAppointments(note)
                    if (aptIndex == null || aptIndex < 0 || aptIndex >= appts.length) return
                    appts[aptIndex].done = !!done
                    try {
                        const res = await fetch(`/api/notes/${note.id}`, {
                            method: 'PUT',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({ appointments: JSON.stringify(appts) })
                        })
                        if (res.ok) {
                            const idx = notes.value.findIndex(n => n.id === note.id)
                            if (idx !== -1) notes.value[idx] = { ...notes.value[idx], appointments: JSON.stringify(appts) }
                            collectOverdueReminders({ skipNotify: true })
                        }
                    } catch (e) {
                        console.error('Fehler beim Aktualisieren des Termins', e)
                    }
                }

                const completeReminderAppointment = async (ri) => {
                    const r = overdueReminders.value[ri]
                    if (!r || r.kind !== 'apt') return
                    await markApptDone(r.note, r.aptIdx, true)
                }

                let startupChecksDone = false
                const runStartupChecks = async () => {
                    if (startupChecksDone) return
                    startupChecksDone = true
                    await autoArchiveDone()
                    if (trashPurgeEnabled.value) {
                        await purgeTrash(trashPurgeDay.value ?? 30)
                    }
                    collectOverdueReminders({ skipNotify: true })
                }

                // --- Countdown timer (header) ---
                const timerOpen = ref(false)
                const timerTotal = ref(0)
                const timerRemaining = ref(0)
                const timerRunning = ref(false)
                let timerInterval = null

                const fmtTimer = () => {
                    const s = Math.max(0, Math.round(timerRemaining.value))
                    return `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`
                }

                const timerSet = (min) => {
                    timerTotal.value = min * 60
                    timerRemaining.value = min * 60
                    timerRunning.value = false
                    clearInterval(timerInterval)
                }

                const timerToggle = () => {
                    if (timerRemaining.value <= 0) {
                        if (timerTotal.value <= 0) timerTotal.value = 600
                        timerRemaining.value = timerTotal.value
                    }
                    timerRunning.value = !timerRunning.value
                    clearInterval(timerInterval)
                    if (timerRunning.value) {
                        timerInterval = setInterval(() => {
                            timerRemaining.value -= 1
                            if (timerRemaining.value <= 0) {
                                timerRemaining.value = 0
                                clearInterval(timerInterval)
                                timerRunning.value = false
                                timerOpen.value = false
                                notify('Notice: Timer abgelaufen', 'Der Countdown ist abgelaufen.')
                            }
                        }, 1000)
                    }
                }

                const timerReset = () => {
                    timerRunning.value = false
                    clearInterval(timerInterval)
                    timerRemaining.value = timerTotal.value
                }

                // Periodic overdue check (every 5 minutes) + notification permission
                let reminderInterval = null
                const startReminderLoop = () => {
                    reminderInterval = setInterval(() => collectOverdueReminders(), 300000)
                    if (typeof Notification !== 'undefined' && Notification.permission === 'default') {
                        Notification.requestPermission().catch(() => {})
                    }
                }

                // --- Drag & drop sorting ---
                const dragCol = ref(null)
                const dragInsertY = ref(null)

                const endDrag = () => {
                    draggedNote.value = null
                    dragCol.value = null
                    dragInsertY.value = null
                }

                const dropPosition = (e) => {
                    const col = e.currentTarget
                    const cont = col.querySelector(':scope > .relative')
                    if (!cont) return { idx: 0, y: 0 }
                    const contRect = cont.getBoundingClientRect()
                    const scroll = cont.scrollTop || 0
                    const cards = [...cont.querySelectorAll('.board-card')]
                        .filter(c => Number(c.dataset.noteId) !== (draggedNote.value && draggedNote.value.id))
                    const viewRects = cards.map(c => c.getBoundingClientRect())
                    let idx = viewRects.length
                    for (let i = 0; i < viewRects.length; i++) {
                        if (e.clientY < viewRects[i].top + viewRects[i].height / 2) { idx = i; break }
                    }
                    const toLocal = (rv) => rv.top - contRect.top + scroll
                    let y
                    if (viewRects.length === 0) y = 0
                    else if (idx === 0) y = Math.max(0, toLocal(viewRects[0]) - 3)
                    else if (idx >= viewRects.length) y = toLocal(viewRects[viewRects.length - 1]) + viewRects[viewRects.length - 1].height + 3
                    else y = (toLocal(viewRects[idx - 1]) + viewRects[idx - 1].height + toLocal(viewRects[idx])) / 2
                    return { idx, y }
                }

                const onColDragOver = (columnId, e) => {
                    dragCol.value = columnId
                    dragInsertY.value = dropPosition(e).y
                }

                const onColDrop = async (columnId, e) => {
                    const dragged = draggedNote.value
                    if (!dragged) return
                    const snapshot = { id: dragged.id, title: dragged.title, status: dragged.status, sort_order: dragged.sort_order }
                    const { idx } = dropPosition(e)
                    if (dragged.status === columnId) {
                        await reorderColumn(columnId, dragged, idx)
                    } else {
                        dragged.status = columnId
                        const maxSort = notes.value
                            .filter(n => n.status === columnId && n.id !== dragged.id)
                            .reduce((max, n) => Math.max(max, n.sort_order || 0), 0)
                        dragged.sort_order = maxSort + 1
                        try {
                            await fetch(`/api/notes/${dragged.id}`, {
                                method: 'PUT',
                                headers: { 'Content-Type': 'application/json' },
                                body: JSON.stringify({
                                    title: dragged.title,
                                    content: dragged.content,
                                    status: dragged.status,
                                    priority: dragged.priority,
                                    due_date: dragged.due_date,
                                    sort_order: dragged.sort_order
                                })
                            })
                        } catch (err) {
                            console.error('Fehler beim Verschieben', err)
                        }
                        await reorderColumn(columnId, dragged, idx)
                    }
                    pushUndo(t('undo.moved', { title: snapshot.title }), async () => {
                        const n = notes.value.find(x => x.id === snapshot.id)
                        if (!n) return
                        n.status = snapshot.status
                        n.sort_order = snapshot.sort_order
                        try {
                            await fetch(`/api/notes/${snapshot.id}`, {
                                method: 'PUT',
                                headers: { 'Content-Type': 'application/json' },
                                body: JSON.stringify({
                                    title: n.title,
                                    content: n.content,
                                    status: snapshot.status,
                                    priority: n.priority,
                                    due_date: n.due_date,
                                    sort_order: snapshot.sort_order
                                })
                            })
                        } catch (err) {
                            console.error('Fehler beim Rückgängig-machen (Verschieben)', err)
                        }
                    })
                    endDrag()
                    collectOverdueReminders()
                }

                const reorderColumn = async (columnId, dragged, to) => {
                    let colNotes = notes.value
                        .filter(n => n.status === columnId)
                        .sort((a, b) => (a.sort_order || 0) - (b.sort_order || 0) || a.id - b.id)

                    // Remove the dragged element
                    const fromIndex = colNotes.findIndex(n => n.id === dragged.id)
                    if (fromIndex !== -1) colNotes.splice(fromIndex, 1)

                    // Target index into the remaining list
                    let toIndex = to
                    if (toIndex === undefined || toIndex === null || toIndex < 0) toIndex = colNotes.length
                    if (toIndex > colNotes.length) toIndex = colNotes.length

                    // Insert
                    colNotes.splice(toIndex, 0, dragged)

                    // Assign new sort_order values
                    const updates = colNotes.map((n, i) => {
                        if ((n.sort_order || 0) !== i || n.id === dragged.id) {
                            return { id: n.id, sort_order: i }
                        }
                        return null
                    }).filter(Boolean)

                    // Update locally
                    colNotes.forEach((n, i) => { n.sort_order = i })
                    dragged.status = columnId

                    // Update the API
                    await Promise.all(updates.map(async (u) => {
                        const n = notes.value.find(x => x.id === u.id)
                        if (!n) return
                        try {
                            await fetch(`/api/notes/${u.id}`, {
                                method: 'PUT',
                                headers: { 'Content-Type': 'application/json' },
                                body: JSON.stringify({
                                    title: n.title,
                                    content: n.content,
                                    status: n.status,
                                    priority: n.priority,
                                    due_date: n.due_date,
                                    sort_order: u.sort_order
                                })
                            })
                        } catch (e) {
                            console.error('Fehler beim Sortieren', e)
                        }
                    }))
                }

                const menuContentText = () => contextMenu.value.source === 'new'
                    ? (newNoteContent.value || '')
                    : (contextMenu.value.source === 'wiki'
                        ? (wikiForm.value.content || '')
                        : (activeNote.value ? (activeNote.value.content || '') : ''))
                const setMenuContentText = (v) => {
                    if (contextMenu.value.source === 'new') newNoteContent.value = v
                    else if (contextMenu.value.source === 'wiki') wikiForm.value.content = v
                    else if (activeNote.value) activeNote.value.content = v
                }
                const menuTextareaEl = () => contextMenu.value.source === 'new' ? newNoteTextareaRef.value
                    : (contextMenu.value.source === 'wiki' ? wikiTextareaRef.value : textareaRef.value)
                const focusMenuTextarea = (start, len) => {
                    nextTick(() => {
                        const ta = menuTextareaEl()
                        if (!ta) return
                        ta.focus()
                        ta.setSelectionRange(start, start + len)
                    })
                }

                const applyFormat = (syntax) => {
                    const start = contextMenu.value.selectionStart
                    const end = contextMenu.value.selectionEnd
                    const text = menuContentText()
                    const selectedText = text.substring(start, end)
                    const replacement = `${syntax}${selectedText || 'Text'}${syntax}`
                    setMenuContentText(text.substring(0, start) + replacement + text.substring(end))
                    closeContextMenu()
                    focusMenuTextarea(start + syntax.length, (selectedText || 'Text').length)
                }

                const applyHtmlColor = (colorHex) => {
                    const start = contextMenu.value.selectionStart
                    const end = contextMenu.value.selectionEnd
                    const text = menuContentText()
                    const selectedText = text.substring(start, end)
                    const openTag = `<span style="color: ${colorHex}">`
                    const replacement = `${openTag}${selectedText || 'Text'}</span>`
                    setMenuContentText(text.substring(0, start) + replacement + text.substring(end))
                    closeContextMenu()
                    focusMenuTextarea(start + openTag.length, (selectedText || 'Text').length)
                }

                const addStep = () => {
                    if (!newStepText.value.trim()) return
                    activeChecklist.value.push({ text: newStepText.value.trim(), done: false })
                    newStepText.value = ''
                }

                const removeStep = (index) => {
                    activeChecklist.value.splice(index, 1)
                }

                let dragStepIndex = null

                const stepDragStart = (index) => {
                    dragStepIndex = index
                }

                const stepDrop = (e, list) => {
                    if (dragStepIndex === null) return
                    const rects = [...e.currentTarget.querySelectorAll('.step-row')]
                        .map(el => el.getBoundingClientRect())
                    let to = rects.length
                    for (let i = 0; i < rects.length; i++) {
                        if (e.clientY < rects[i].top + rects[i].height / 2) { to = i; break }
                    }
                    const from = dragStepIndex
                    dragStepIndex = null
                    if (from === to || from === to - 1) return
                    const [moved] = list.splice(from, 1)
                    list.splice(from < to ? to - 1 : to, 0, moved)
                }

                const centerBlocks = (md) => {
                    return md.replace(/^:::center[ \t]*\n([\s\S]*?)\n:::[ \t]*$/gm, (match, inner) => {
                        return `<div class="md-center">${marked.parse(inner).trim()}</div>`
                    })
                }

                const sanitizeHtml = (html) => {
                    const template = document.createElement('template')
                    template.innerHTML = html
                    const DISALLOWED = new Set(['script', 'style', 'iframe', 'object', 'embed', 'meta', 'link', 'form', 'textarea', 'select', 'option', 'video', 'audio', 'source', 'track', 'canvas', 'svg', 'math', 'template', 'title', 'base', 'applet', 'frame', 'frameset', 'noframes', 'noscript'])
                    const UNWRAP = new Set(['header', 'footer', 'aside', 'section', 'article', 'nav', 'main', 'figure', 'figcaption', 'summary', 'details', 'mark', 'small', 'sub', 'sup', 'kbd', 'samp', 'var', 'q', 'cite', 'abbr', 'time', 'ins', 'u', 'b', 'i', 'font', 'center', 'div2', 'span2'])
                    const ALLOWED_ATTRS = {
                        'A': new Set(['class', 'data-note-title', 'data-address-name', 'data-wiki-title', 'href', 'target', 'rel']),
                        'IMG': new Set(['src', 'alt', 'title']),
                        'SPAN': new Set(['style', 'class']),
                        'DIV': new Set(['class']),
                        'INPUT': new Set(['type', 'data-cb-i', 'checked']),
                        'TH': new Set(['align']),
                        'TD': new Set(['align']),
                        'TABLE': new Set(['align']),
                        'P': new Set(['class', 'align']),
                        'H1': new Set(['class', 'align']),
                        'H2': new Set(['class', 'align']),
                        'H3': new Set(['class', 'align']),
                        'H4': new Set(['class', 'align']),
                        'H5': new Set(['class', 'align']),
                        'H6': new Set(['class', 'align'])
                    }
                    const process = (node) => {
                        if (node.nodeType === Node.COMMENT_NODE) {
                            node.remove()
                            return
                        }
                        if (node.nodeType !== Node.ELEMENT_NODE) return
                        const tag = node.tagName.toLowerCase()
                        if (DISALLOWED.has(tag)) {
                            node.remove()
                            return
                        }
                        const attrAllow = ALLOWED_ATTRS[node.tagName] || new Set(['class'])
                        ;[...node.attributes].forEach(attr => {
                            if (!attrAllow.has(attr.name)) node.removeAttribute(attr.name)
                        })
                        if (node.tagName === 'A') {
                            node.setAttribute('target', '_blank')
                            node.setAttribute('rel', 'noopener noreferrer')
                            const href = node.getAttribute('href')
                            if (href) {
                                const scheme = href.replace(/^([a-zA-Z][a-zA-Z0-9+.-]*):.*$/, '$1').toLowerCase()
                                if (!['http', 'https', 'mailto'].includes(scheme) && !href.startsWith('/') && !href.startsWith('#')) {
                                    node.removeAttribute('href')
                                }
                            }
                        }
                        if (node.tagName === 'IMG') {
                            const src = node.getAttribute('src')
                            if (src && !/^(data:image\/|https?:\/\/|\/)/i.test(src)) node.removeAttribute('src')
                        }
                        if (node.tagName === 'SPAN') {
                            const style = node.getAttribute('style')
                            if (style && !/^color\s*:/i.test(style.trim())) node.removeAttribute('style')
                        }
                        if (node.tagName === 'DIV') {
                            const cls = node.getAttribute('class') || ''
                            if (cls.trim() !== 'md-center') {
                                const children = [...node.childNodes]
                                node.replaceWith(...children)
                                children.forEach(process)
                                return
                            }
                        }
                        if (UNWRAP.has(tag)) {
                            const children = [...node.childNodes]
                            node.replaceWith(...children)
                            children.forEach(process)
                            return
                        }
                        [...node.childNodes].forEach(process)
                    }
                    [...template.content.childNodes].forEach(process)
                    return template.innerHTML
                }

                const renderMarkdown = (content, checklist) => {
                    if (!content) return '<p class="text-zinc-600 dark:text-zinc-500">' + t('wiki.no_content') + '</p>'
                    let html = marked.parse(centerBlocks(content))
                    html = processAddressLinks(processNoteLinks(processWikiLinks(html)))
                    let idx = 0
                    html = html.replace(/<input[^>]*disabled[^>]*type="checkbox"[^>]*>/g, () => {
                        const cur = idx++
                        const done = checklist && checklist[cur] ? checklist[cur].done : false
                        return `<input type="checkbox" data-cb-i="${cur}"${done ? ' checked' : ''}>`
                    })
                    return sanitizeHtml(html)
                }

                const renderedMarkdown = computed(() => {
                    if (!activeNote.value) return '<p class="text-zinc-600 dark:text-zinc-500">' + t('wiki.no_content') + '</p>'
                    return renderMarkdown(activeNote.value.content, activeChecklist.value)
                })

                const newNoteRenderedMarkdown = computed(() => {
                    return renderMarkdown(newNoteContent.value, newNoteChecklist.value)
                })

                const onChecklistToggle = (e, list) => {
                    const cb = e.target.closest('input[data-cb-i]')
                    if (cb) {
                        const i = parseInt(cb.getAttribute('data-cb-i'), 10)
                        if (list && list[i]) list[i].done = !list[i].done
                        return true
                    }
                    return false
                }

                const onPreviewClick = (e) => {
                    if (onChecklistToggle(e, activeChecklist.value)) return
                    handleNoteLinkClick(e)
                }

                const onNewNotePreviewClick = (e) => {
                    if (onChecklistToggle(e, newNoteChecklist.value)) return
                    handleNoteLinkClick(e)
                }

                const toggleDetailPreview = () => {
                    isPreviewMode.value = !isPreviewMode.value
                    showAutocomplete.value = false
                }
                const toggleNewNotePreview = () => {
                    newNotePreviewMode.value = !newNotePreviewMode.value
                    showAutocomplete.value = false
                }

                const resizePastedImage = (file, maxWidth) => {
                    return new Promise((resolve, reject) => {
                        const reader = new FileReader()
                        reader.onload = () => {
                            const img = new Image()
                            img.onload = () => {
                                const scale = Math.min(1, maxWidth / (img.width || maxWidth))
                                const w = Math.max(1, Math.round((img.width || 1) * scale))
                                const h = Math.max(1, Math.round((img.height || 1) * scale))
                                const canvas = document.createElement('canvas')
                                canvas.width = w
                                canvas.height = h
                                canvas.getContext('2d').drawImage(img, 0, 0, w, h)
                                resolve(canvas.toDataURL('image/jpeg', 0.85))
                            }
                            img.onerror = reject
                            img.src = reader.result
                        }
                        reader.onerror = reject
                        reader.readAsDataURL(file)
                    })
                }

                const insertImageFile = async (file, which, start, end) => {
                    if (!file || !file.type || !file.type.startsWith('image/')) return
                    let markdownImage
                    try {
                        const dataUrl = await resizePastedImage(file, 1200)
                        markdownImage = `![Bild](${dataUrl})`
                    } catch (err) {
                        console.error('Fehler beim Bild-Einfügen', err)
                        return
                    }
                    const cur = which === 'new' ? (newNoteContent.value || '')
                    : (which === 'wiki' ? (wikiForm.value.content || '') : (activeNote.value ? (activeNote.value.content || '') : ''))
                    const next = cur.substring(0, start) + markdownImage + cur.substring(end)
                    if (which === 'new') newNoteContent.value = next
                    else if (which === 'wiki') wikiForm.value.content = next
                    else if (activeNote.value) activeNote.value.content = next
                    closeContextMenu()
                    await nextTick()
                    const ta = which === 'new' ? newNoteTextareaRef.value
                        : (which === 'wiki' ? wikiTextareaRef.value : textareaRef.value)
                    if (!ta) return
                    ta.focus()
                    const pos = start + markdownImage.length
                    ta.setSelectionRange(pos, pos)
                }

                const onEditorPaste = async (e, which) => {
                    const ta = which === 'new' ? newNoteTextareaRef.value
                        : (which === 'wiki' ? wikiTextareaRef.value : textareaRef.value)
                    if (!ta) return
                    const items = (e.clipboardData && e.clipboardData.items) || []
                    for (const item of items) {
                        if (item.kind === 'file' && item.type && item.type.startsWith('image/')) {
                            const file = item.getAsFile()
                            if (!file) return
                            e.preventDefault()
                            await insertImageFile(file, which, ta.selectionStart, ta.selectionEnd)
                            return
                        }
                    }
                }

                const triggerImageUpload = () => {
                    if (imageUploadInputRef.value) imageUploadInputRef.value.click()
                }

                const onImageFileSelected = async (e) => {
                    const file = e.target.files && e.target.files[0]
                    e.target.value = ''
                    if (!file) return
                    await insertImageFile(file, contextMenu.value.source, contextMenu.value.selectionStart, contextMenu.value.selectionEnd)
                }

                const saveActiveNote = async () => {
                    if (!activeNote.value) return

                    const storagePayload = {
                        text: activeNote.value.content,
                        checklist: activeChecklist.value
                    };
                    const serializedContent = JSON.stringify(storagePayload);

                    try {
                        const res = await fetch(`/api/notes/${activeNote.value.id}`, {
                            method: 'PUT',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({
                                title: activeNote.value.title,
                                content: serializedContent,
                                status: activeNote.value.status,
                                priority: activeNote.value.priority,
                                due_date: activeNote.value.due_date || null,
                                departments: JSON.stringify(activeNoteDepartments.value),
                                appointments: JSON.stringify(activeNoteAppointments.value),
                                repeat_rule: activeNote.value.repeat_rule || null
                            })
                        })
                        if (res.ok) {
                            const index = notes.value.findIndex(n => n.id === activeNote.value.id)
                            if (index !== -1) {
                                notes.value[index] = { ...activeNote.value, content: serializedContent, departments: JSON.stringify(activeNoteDepartments.value), appointments: JSON.stringify(activeNoteAppointments.value) }
                            }
                            fetchDepartments()
                            collectOverdueReminders({ skipNotify: true })
                            closeModal()
                        }
                    } catch (e) {
                        console.error('Fehler beim Aktualisieren', e)
                    }
                }

                const deleteNote = async (id) => {
                    const note = notes.value.find(n => n.id === id)
                    try {
                        const res = await fetch(`/api/notes/${id}`, { method: 'DELETE' })
                        if (res.ok) {
                            notes.value = notes.value.filter(n => n.id !== id)
                            fetchTrash()
                            if (note) {
                                pushUndo(t('undo.deleted', { title: note.title }), async () => {
                                    await fetch(`/api/notes/${id}/restore`, { method: 'POST' })
                                    notes.value.push({ ...note, deleted_at: null })
                                    notes.value.sort((a, b) => (a.sort_order || 0) - (b.sort_order || 0) || a.id - b.id)
                                    fetchTrash()
                                })
                            }
                        }
                    } catch (e) {
                        console.error('Fehler beim Löschen', e)
                    }
                }

                // --- Search syntax: key:value parser ---
                const STATUS_ALIASES = {
                    'backlog': 'backlog',
                    'in_progress': 'in_progress',
                    'inprogress': 'in_progress',
                    'in-progress': 'in_progress',
                    'in arbeit': 'in_progress',
                    'review': 'review',
                    'done': 'done',
                    'abgeschlossen': 'done',
                    'archived': 'archived',
                    'archiv': 'archived'
                }

                const PRIO_ALIASES = {
                    'high': 'high',
                    'h': 'high',
                    'hoch': 'high',
                    'medium': 'medium',
                    'm': 'medium',
                    'mittel': 'medium',
                    'low': 'low',
                    'l': 'low',
                    'niedrig': 'low'
                }

                const parseFlexDate = (str) => {
                    // YYYY-MM-DD
                    let m = str.match(/^(\d{4})-(\d{2})-(\d{2})$/)
                    if (m) return new Date(+m[1], +m[2] - 1, +m[3]).getTime()
                    // DD.MM.YYYY
                    m = str.match(/^(\d{2})\.(\d{2})\.(\d{4})$/)
                    if (m) return new Date(+m[3], +m[2] - 1, +m[1]).getTime()
                    // YYYY-MM-DD tolerant to leading zeros being omitted
                    m = str.match(/^(\d{4})-(\d{1,2})-(\d{1,2})$/)
                    if (m) return new Date(+m[1], +m[2] - 1, +m[3]).getTime()
                    return null
                }

                const startOfDay = (t) => new Date(new Date(t).setHours(0, 0, 0, 0)).getTime()
                const endOfDay = (t) => new Date(new Date(t).setHours(23, 59, 59, 999)).getTime()

                const parseSearchQuery = (raw) => {
                    const result = {
                        freeText: [],
                        titel: [],
                        inhalt: [],
                        status: [],
                        prio: [],
                        datum: [],
                        faellig: [],
                        department: []
                    }
                    // Detect key:value tokens (value may contain ".." or date characters, no whitespace)
                    const tokens = raw.match(/(\w+):([^\s]+)/g) || []
                    let rest = raw
                    tokens.forEach(tok => {
                        const idx = rest.indexOf(tok)
                        const valStart = tok.indexOf(':') + 1
                        const val = tok.substring(valStart)
                        const key = tok.substring(0, tok.indexOf(':')).toLowerCase()
                        // Strip the matched token from the remainder for free-text matching
                        rest = rest.replace(tok, ' ')
                        if (key === 'titel' || key === 'title') result.titel.push(val)
                        else if (key === 'inhalt' || key === 'content') result.inhalt.push(val)
                        else if (key === 'status') result.status.push(val)
                        else if (key === 'prio' || key === 'priority') result.prio.push(val)
                        else if (key === 'datum' || key === 'date') result.datum.push(val)
                        else if (key === 'faellig' || key === 'due' || key === 'due_date' || key === 'fällig') result.faellig.push(val)
                        else if (key === 'dep' || key === 'department') result.department.push(val)
                        else result.freeText.push(tok)
                    })
                    // Remaining free-text tokens
                    const freeTokens = rest.split(/\s+/).filter(t => t.trim() !== '')
                    result.freeText.push(...freeTokens)
                    return result
                }

                const evalDateSpec = (fieldVal, spec) => {
                    const today = startOfDay(Date.now())
                    const endToday = endOfDay(Date.now())
                    if (spec === 'none') return !fieldVal || fieldVal.trim() === ''
                    if (spec === 'any') return !!fieldVal && fieldVal.trim() !== ''
                    if (!fieldVal || fieldVal.trim() === '') return false

                    const valTime = parseFlexDate(fieldVal)
                    if (valTime === null) return false

                    if (spec === 'today') return valTime >= today && valTime <= endToday
                    if (spec === 'overdue') return valTime < today
                    if (spec === 'this_week' || spec === 'thisweek' || spec === 'woche') {
                        const now = new Date()
                        const day = (now.getDay() + 6) % 7
                        const monday = startOfDay(new Date(now).setDate(now.getDate() - day))
                        const sunday = endOfDay(new Date(now).setDate(now.getDate() - day + 6))
                        return valTime >= monday && valTime <= sunday
                    }
                    if (spec.startsWith('>=')) {
                        const t = parseFlexDate(spec.substring(2))
                        return t !== null && valTime >= startOfDay(t)
                    }
                    if (spec.startsWith('<=')) {
                        const t = parseFlexDate(spec.substring(2))
                        return t !== null && valTime <= endOfDay(t)
                    }
                    if (spec.startsWith('>')) {
                        const t = parseFlexDate(spec.substring(1))
                        return t !== null && valTime > endOfDay(t)
                    }
                    if (spec.startsWith('<')) {
                        const t = parseFlexDate(spec.substring(1))
                        return t !== null && valTime < startOfDay(t)
                    }
                    if (spec.includes('..')) {
                        const [a, b] = spec.split('..')
                        const tA = parseFlexDate(a)
                        const tB = parseFlexDate(b)
                        if (tA === null || tB === null) return false
                        return valTime >= startOfDay(tA) && valTime <= endOfDay(tB)
                    }
                    const t = parseFlexDate(spec)
                    if (t === null) return false
                    return valTime >= startOfDay(t) && valTime <= endOfDay(t)
                }

                const notePlainContent = (note) => {
                    try {
                        const parsed = JSON.parse(note.content)
                        if (parsed && typeof parsed === 'object' && parsed.text !== undefined) return String(parsed.text)
                    } catch {}
                    return note.content || ''
                }

                const getDepartments = (note) => {
                    if (!note) return []
                    try {
                        const raw = note.departments
                        if (!raw) {
                            if (note.department && note.department.trim()) return [note.department]
                            return []
                        }
                        const parsed = JSON.parse(raw)
                        if (Array.isArray(parsed)) return parsed.filter(d => d && d.trim() !== '')
                        return []
                    } catch {
                        if (note.department && note.department.trim()) return [note.department]
                        return []
                    }
                }

                const getAppointments = (note) => {
                    if (!note) return []
                    try {
                        const raw = note.appointments
                        if (!raw) return []
                        const parsed = JSON.parse(raw)
                        if (Array.isArray(parsed)) return parsed
                        return []
                    } catch {
                        return []
                    }
                }

                const getChecklistProgress = (note) => {
                    if (!note) return null
                    try {
                        const parsed = JSON.parse(note.content)
                        const checklist = (parsed && parsed.checklist) ? parsed.checklist : []
                        if (!Array.isArray(checklist) || checklist.length === 0) return null
                        const done = checklist.filter(s => s.done).length
                        return { done, total: checklist.length }
                    } catch {
                        return null
                    }
                }

                const matchesNote = (note, q) => {
                    if (q.freeText.length) {
                        const depts = getDepartments(note).join(' ')
                        const haystack = (note.title + ' ' + depts + ' ' + notePlainContent(note)).toLowerCase()
                        if (!q.freeText.every(t => haystack.includes(t.toLowerCase()))) return false
                    }
                    if (q.titel.length) {
                        const title = note.title.toLowerCase()
                        if (!q.titel.every(t => title.includes(t.toLowerCase()))) return false
                    }
                    if (q.inhalt.length) {
                        const content = notePlainContent(note).toLowerCase()
                        if (!q.inhalt.every(t => content.includes(t.toLowerCase()))) return false
                    }
                    if (q.department.length) {
                        const depts = getDepartments(note).join(' ').toLowerCase()
                        if (!q.department.every(t => depts.includes(t.toLowerCase()))) return false
                    }
                    if (q.status.length) {
                        const resolved = q.status.map(s => STATUS_ALIASES[s.trim().toLowerCase()]).filter(Boolean)
                        if (resolved.length && !resolved.includes(note.status)) return false
                    }
                    if (q.prio.length) {
                        const resolved = q.prio.map(p => PRIO_ALIASES[p.trim().toLowerCase()]).filter(Boolean)
                        if (resolved.length && !resolved.includes(note.priority)) return false
                    }
                    if (q.datum.length) {
                        if (!q.datum.every(d => evalDateSpec(note.date, d))) return false
                    }
                    if (q.faellig.length) {
                        if (!q.faellig.every(d => evalDateSpec(note.due_date, d))) return false
                    }
                    return true
                }

                // --- Calendar logic ---
                const dateStr = (d) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`

                const convertDateToISO = (v) => {
                    if (!v) return null
                    const m = String(v).match(/^(\d{1,2})\.(\d{1,2})\.(\d{4})$/)
                    if (m) return `${m[3]}-${String(m[2]).padStart(2, '0')}-${String(m[1]).padStart(2, '0')}`
                    if (/^\d{4}-\d{2}-\d{2}$/.test(String(v))) return String(v)
                    return null
                }

                const fmtDate = (v) => {
                    if (!v) return ''
                    const m = String(v).match(/^(\d{4})-(\d{2})-(\d{2})/)
                    if (!m) return String(v)
                    return new Date(+m[1], +m[2] - 1, +m[3]).toLocaleDateString(locale(), { year: 'numeric', month: '2-digit', day: '2-digit' })
                }

                const fmtAptRange = (ev) => {
                    let s = fmtDate(ev.start) + (ev.time ? ' ' + ev.time : '')
                    if (ev.end) s += ' – ' + fmtDate(ev.end) + (ev.endTime ? ' ' + ev.endTime : '')
                    return s
                }

                const calEventsForDate = (d) => {
                    const ds = dateStr(d)
                    const events = []
                    notes.value.forEach(note => {
                        if (note.status === 'archived') return
                        const appts = getAppointments(note)
                        appts.forEach(apt => {
                            if (apt.done || !apt.start) return
                            if (apt.start === ds || (apt.end >= ds && apt.start <= ds)) {
                                const overdue = apt.start < ds && !apt.end
                                events.push({
                                    title: `${note.title} – ${apt.title || t('reminder.apt')}`,
                                    aptTitle: apt.title || t('reminder.apt'),
                                    noteTitle: note.title,
                                    time: apt.time || '',
                                    cls: overdue ? 'cal-event-overdue' : 'cal-event-apt',
                                    source: note.title,
                                    noteId: note.id,
                                    departments: getDepartments(note),
                                    start: apt.start,
                                    end: apt.end || '',
                                    endTime: apt.endTime || ''
                                })
                            }
                        })
                        if (note.due_date && note.due_date === ds) {
                            const overdue = note.due_date < dateStr(new Date())
                            events.push({
                                title: t('reminder.due') + ': ' + note.title,
                                aptTitle: t('reminder.due'),
                                noteTitle: note.title,
                                time: '',
                                cls: overdue ? 'cal-event-overdue' : 'cal-event-due',
                                source: note.title,
                                noteId: note.id,
                                departments: getDepartments(note)
                            })
                        }
                    })
                    events.sort((a, b) => (a.time || '').localeCompare(b.time || ''))
                    return events
                }

                const calDayStruct = (date, currentMonth) => {
                    const events = calEventsForDate(date)
                    const ds = dateStr(date)
                    const d = new Date(date)
                    const todayStr = dateStr(new Date())
                    const overdueStr = { cls: 'bg-red-500 rounded-full' }
                    const aptStr = { cls: 'bg-emerald-500 rounded-full' }
                    const dueStr = { cls: 'bg-blue-500 rounded-full' }
                    const dots = events.map(ev => {
                        if (ev.cls === 'cal-event-overdue') return overdueStr
                        if (ev.cls === 'cal-event-apt') return aptStr
                        return dueStr
                    })
                    let frameCls = ''
                    if (events.length > 0) {
                        let prio = 0
                        for (const ev of events) {
                            if (ev.cls === 'cal-event-overdue') prio = Math.max(prio, 3)
                            else if (ev.cls === 'cal-event-apt') prio = Math.max(prio, 2)
                            else prio = Math.max(prio, 1)
                        }
                        frameCls = prio === 3 ? 'ring-1 ring-red-400 dark:ring-red-500' :
                            prio === 2 ? 'ring-1 ring-emerald-400 dark:ring-emerald-500' :
                            'ring-1 ring-blue-400 dark:ring-blue-500'
                    }
                    return {
                        date: d,
                        currentMonth,
                        isToday: ds === todayStr,
                        dots,
                        events,
                        frameCls
                    }
                }

                const calMonthDays = computed(() => {
                    const now = calCursor.value
                    const year = now.getFullYear()
                    const month = now.getMonth()
                    const first = new Date(year, month, 1)
                    const startDay = (first.getDay() + 6) % 7
                    const daysInMonth = new Date(year, month + 1, 0).getDate()
                    const start = new Date(year, month, 1 - startDay)
                    const result = []
                    for (let i = 0; i < 42; i++) {
                        const d = new Date(start)
                        d.setDate(start.getDate() + i)
                        result.push(calDayStruct(d, d.getMonth() === month))
                    }
                    return result
                })

                const calHours = Array.from({ length: 25 }, (_, h) => h)

                const calWeekBlocks = (events, ds) => {
                    const allday = []
                    const timed = []
                    events.forEach(ev => {
                        let startMin = -1
                        let endMin = -1
                        const stime = ev.time || ''
                        if (/^\d{1,2}:\d{2}$/.test(stime)) {
                            const isMulti = ev.start && ev.end && ev.end > ev.start
                            const isStartDay = ds === ev.start
                            const isEndDay = isMulti && ds === ev.end
                            if (isStartDay) {
                                const hm = stime.split(':')
                                startMin = parseInt(hm[0], 10) * 60 + parseInt(hm[1], 10)
                                if (isMulti) {
                                    endMin = 1440
                                } else if (ev.endTime && /^\d{1,2}:\d{2}$/.test(ev.endTime)) {
                                    const ehm = ev.endTime.split(':')
                                    endMin = parseInt(ehm[0], 10) * 60 + parseInt(ehm[1], 10)
                                } else {
                                    endMin = Math.min(1440, startMin + 60)
                                }
                            } else if (isEndDay) {
                                startMin = 0
                                if (ev.endTime && /^\d{1,2}:\d{2}$/.test(ev.endTime)) {
                                    const ehm = ev.endTime.split(':')
                                    endMin = parseInt(ehm[0], 10) * 60 + parseInt(ehm[1], 10)
                                } else {
                                    endMin = 1440
                                }
                            } else if (isMulti) {
                                startMin = 0
                                endMin = 1440
                            } else {
                                const hm = stime.split(':')
                                startMin = parseInt(hm[0], 10) * 60 + parseInt(hm[1], 10)
                                if (ev.endTime && /^\d{1,2}:\d{2}$/.test(ev.endTime)) {
                                    const ehm = ev.endTime.split(':')
                                    endMin = parseInt(ehm[0], 10) * 60 + parseInt(ehm[1], 10)
                                } else {
                                    endMin = Math.min(1440, startMin + 60)
                                }
                            }
                        }
                        if (startMin < 0) {
                            allday.push(ev)
                        } else {
                            timed.push({ ev, startMin, endMin: Math.max(endMin, startMin + 15) })
                        }
                    })
                    // Overlap nesting: assign lanes greedily by start time.
                    timed.sort((a, b) => a.startMin - b.startMin || b.endMin - a.endMin)
                    const lanes = []  // lanes[i] = { endMin }
                    let laneCount = 0
                    for (const t of timed) {
                        let placed = lanes.findIndex(l => t.startMin >= l.endMin)
                        if (placed < 0) {
                            placed = lanes.length
                            lanes.push({ endMin: 0 })
                            laneCount = Math.max(laneCount, placed + 1)
                        }
                        lanes[placed] = { endMin: t.endMin }
                        t.lane = placed
                    }
                    const blocks = timed.map(t => {
                        const widthPct = laneCount > 0 ? (100 / laneCount) : 100
                        return {
                            ev: t.ev,
                            cls: t.ev.cls,
                            title: t.ev.title,
                            aptTitle: t.ev.aptTitle,
                            noteTitle: t.ev.noteTitle,
                            time: (t.startMin > 0 || (t.ev.end && t.ev.end === ds)) ? t.ev.time : '',
                            noteId: t.ev.noteId,
                            departments: t.ev.departments || [],
                            startMin: t.startMin,
                            heightMin: t.endMin - t.startMin,
                            lane: t.lane,
                            laneCount,
                            leftPct: (t.lane / laneCount) * 100,
                            widthPct
                        }
                    })
                    return { allday, blocks, laneCount }
                }

                const dowShorts = () => {
                    const now = calCursor.value
                    const dayOfWeek = (now.getDay() + 6) % 7
                    const start = new Date(now)
                    start.setDate(now.getDate() - dayOfWeek)
                    const fmt = new Intl.DateTimeFormat(locale(), { weekday: 'short' })
                    return Array.from({ length: 7 }, (_, i) => {
                        const d = new Date(start)
                        d.setDate(start.getDate() + i)
                        return fmt.format(d)
                    })
                }

                const calWeekDays = computed(() => {
                    const now = calCursor.value
                    const dayOfWeek = (now.getDay() + 6) % 7
                    const start = new Date(now)
                    start.setDate(now.getDate() - dayOfWeek)
                    const fmt = new Intl.DateTimeFormat(locale(), { weekday: 'short' })
                    const result = []
                    for (let i = 0; i < 7; i++) {
                        const d = new Date(start)
                        d.setDate(start.getDate() + i)
                        const ds = dateStr(d)
                        const events = calEventsForDate(d)
                        const { allday, blocks, laneCount } = calWeekBlocks(events, ds)
                        result.push({
                            date: d,
                            isToday: ds === dateStr(new Date()),
                            names: fmt.format(d),
                            events,
                            allday,
                            blocks,
                            laneCount,
                            dateStr: ds
                        })
                    }
                    return result
                })

                // Week view: auto-scroll the timeline to the current hour on (re)mount of the view
                const calWeekScrollRef = ref(null)
                const scrollWeekToNow = () => {
                    const apply = () => {
                        if (calWeekScrollRef.value) {
                            const h = new Date().getHours() + 1
                            calWeekScrollRef.value.scrollTop = Math.max(0, h * 36 - 100)
                        }
                    }
                    nextTick(apply)
                    requestAnimationFrame(apply)
                    setTimeout(apply, 250)
                }

                // Week view mobile: single day via prev/next arrows
                const calWeekDayIndex = ref((new Date().getDay() + 6) % 7)
                const calWeekShift = (delta) => {
                    const idx = calWeekDayIndex.value + delta
                    if (delta < 0 && idx < 0) {
                        const prev = new Date(calCursor.value)
                        prev.setDate(prev.getDate() - 7)
                        calCursor.value = prev
                        calWeekDayIndex.value = 6
                    } else if (delta > 0 && idx > 6) {
                        const next = new Date(calCursor.value)
                        next.setDate(next.getDate() + 7)
                        calCursor.value = next
                        calWeekDayIndex.value = 0
                    } else {
                        calWeekDayIndex.value = idx
                    }
                }

                // Month view: drag & drop to move appointments between days
                const calDragEv = ref(null)
                const addDaysToISO = (iso, delta) => {
                    const d = new Date(iso + 'T00:00:00')
                    d.setDate(d.getDate() + delta)
                    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
                }
                const calEventDragStart = (ev, e) => {
                    if (!ev.start) return
                    calDragEv.value = { noteId: ev.noteId, aptTitle: ev.aptTitle, start: ev.start, end: ev.end || '' }
                    try { e.dataTransfer.setData('text/plain', ev.title || '') } catch (e2) {}
                }
                const calDayDrop = async (day) => {
                    const drag = calDragEv.value
                    calDragEv.value = null
                    if (!drag) return
                    const targetStr = dateStr(day.date)
                    if (drag.start === targetStr) return
                    const delta = Math.round((new Date(targetStr + 'T00:00:00') - new Date(drag.start + 'T00:00:00')) / 86400000)
                    if (delta === 0) return
                    const note = notes.value.find(n => n.id === drag.noteId)
                    if (!note) return
                    const appts = getAppointments(note)
                    const apt = appts.find(a => (a.title || '') === drag.aptTitle && (a.start || '') === drag.start)
                    if (!apt) return
                    apt.start = addDaysToISO(apt.start, delta)
                    if (apt.end) apt.end = addDaysToISO(apt.end, delta)
                    const res = await fetch(`/api/notes/${note.id}`, {
                        method: 'PUT',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({ appointments: JSON.stringify(appts) })
                    })
                    if (res.ok) {
                        note.appointments = JSON.stringify(appts)
                        collectOverdueReminders()
                    }
                }

                const calYearMonths = computed(() => {
                    const year = calCursor.value.getFullYear()
                    const months = []
                    for (let m = 0; m < 12; m++) {
                        const first = new Date(year, m, 1)
                        const startDay = (first.getDay() + 6) % 7
                        const daysInMonth = new Date(year, m + 1, 0).getDate()
                        const start = new Date(year, m, 1 - startDay)
                        const days = []
                        for (let i = 0; i < 42; i++) {
                            const d = new Date(start)
                            d.setDate(start.getDate() + i)
                            days.push(calDayStruct(d, d.getMonth() === m))
                        }
                        months.push({ name: first.toLocaleDateString(locale(), { month: 'long' }), days })
                    }
                    return months
                })

                const calTitle = computed(() => {
                    if (calViewMode.value === 'month') {
                        return calCursor.value.toLocaleDateString(locale(), { month: 'long', year: 'numeric' })
                    } else if (calViewMode.value === 'week') {
                        const start = calWeekDays.value[0]
                        const end = calWeekDays.value[6]
                        if (start.date.getMonth() === end.date.getMonth()) {
                            return `${start.date.getDate()}. – ${end.date.getDate()}. ${end.date.toLocaleDateString(locale(), { month: 'long', year: 'numeric' })}`
                        }
                        return `${start.date.getDate()}. ${start.date.toLocaleDateString(locale(), { month: 'short' })} – ${end.date.getDate()}. ${end.date.toLocaleDateString(locale(), { month: 'short', year: 'numeric' })}`
                    }
                    return String(calCursor.value.getFullYear())
                })

                const calToday = () => { calCursor.value = new Date() }

                const calNavigate = (dir) => {
                    const c = new Date(calCursor.value)
                    if (calViewMode.value === 'month') c.setMonth(c.getMonth() + dir)
                    else if (calViewMode.value === 'week') c.setDate(c.getDate() + (7 * dir))
                    else if (calViewMode.value === 'year') c.setFullYear(c.getFullYear() + dir)
                    calCursor.value = c
                }

                const calDayClick = (day) => {
                    calSelectedDay.value = day.date
                }

                const calSelectedDayTitle = computed(() => {
                    if (!calSelectedDay.value) return ''
                    return calSelectedDay.value.toLocaleDateString(locale(), { weekday: 'long', day: '2-digit', month: 'long', year: 'numeric' })
                })

                const calSelectedDayEvents = computed(() => {
                    if (!calSelectedDay.value) return []
                    return calEventsForDate(calSelectedDay.value)
                })

                const boardVisibility = ref('all')
                const boardDept = ref('')

                const getNotesByColumn = (status) => {
                    let filtered = notes.value.filter(note => note.status === status)
                    if (boardVisibility.value === 'overdue') filtered = filtered.filter(isOverdue)
                    else if (boardVisibility.value === 'due') filtered = filtered.filter(note => note.due_date)
                    else if (boardVisibility.value === 'nodate') filtered = filtered.filter(note => !note.due_date)
                    if (boardDept.value) filtered = filtered.filter(note => getDepartments(note).includes(boardDept.value))
                    filtered.sort((a, b) => (a.sort_order || 0) - (b.sort_order || 0) || a.id - b.id)
                    if (searchQuery.value.trim() !== '') {
                        const q = parseSearchQuery(searchQuery.value)
                        filtered = filtered.filter(note => matchesNote(note, q))
                    }
                    return filtered
                }

                const startDrag = (note) => {
                    draggedNote.value = note
                    dragCol.value = null
                    dragInsertY.value = null
                }

                // --- Graph view (Nodemap) ---
                const graphTypes = [
                    { id: 'note', label: 'graph.type_note', color: '#3b82f6' },
                    { id: 'contact', label: 'graph.type_contact', color: '#10b981' },
                    { id: 'wiki', label: 'graph.type_wiki', color: '#8b5cf6' },
                    { id: 'dept', label: 'graph.type_dept', color: '#f59e0b' }
                ]
                const graphTypeColor = (type) => (graphTypes.find(t => t.id === type) || { color: '#71717a' }).color
                const graphTypeLabel = (type) => {
                    const g = graphTypes.find(t => t.id === type)
                    return g ? t(g.label) : type
                }
                const graphTypeVisible = ref({ note: true, contact: true, wiki: true, dept: true })
                const graphNodeData = ref([])
                const graphLinkData = ref([])
                const graphCounts = ref({ note: 0, contact: 0, wiki: 0, dept: 0 })
                const graphHoverId = ref(null)
                const graphSelectedId = ref(null)
                const graphSelected = computed(() => (graphNodeData.value.find(n => n.id === graphSelectedId.value)) || null)
                const graphCanvasRef = ref(null)
                const graphContainerRef = ref(null)
                const graphHint = computed(() => t('graph.hint'))
                const graphPositions = {}
                const graphCam = { x: 0, y: 0, zoom: 1 }
                let graphRAF = null
                let graphDragNode = null
                let graphDraggedDist = 0
                let graphPanning = false
                let graphPanStart = null

                const graphByNodeId = () => {
                    const m = {}
                    graphNodeData.value.forEach(n => { m[n.id] = n })
                    return m
                }

                const rebuildGraph = () => {
                    const nodes = []
                    const links = []
                    const ids = {}
                    const ensure = (type, key, label) => {
                        const id = type + '|' + key
                        let n = ids[id]
                        if (!n) {
                            const prev = graphPositions[id]
                            n = {
                                id,
                                type,
                                key,
                                label: label || key,
                                x: prev ? prev.x : (Math.random() - 0.5) * 600,
                                y: prev ? prev.y : (Math.random() - 0.5) * 600,
                                vx: 0,
                                vy: 0
                            }
                            ids[id] = n
                            nodes.push(n)
                        }
                        return n
                    }
                    const link = (a, b) => {
                        if (!a || !b || a === b) return
                        if (links.some(l => (l.a === a && l.b === b) || (l.a === b && l.b === a))) return
                        links.push({ a, b })
                    }
                    departments.value.forEach(d => ensure('dept', d.name, d.name))
                    notes.value.forEach(note => {
                        const n = ensure('note', String(note.id), note.title)
                        getDepartments(note).forEach(d => link(n, ensure('dept', d, d)))
                        let content = ''
                        try {
                            const parsed = JSON.parse(note.content)
                            if (parsed && typeof parsed === 'object' && parsed.text !== undefined) content = String(parsed.text)
                        } catch { content = note.content || '' }
                        if (!content) return
                        let m
                        const mNote = /\[\[([^\]|]+)/g
                        while ((m = mNote.exec(content))) {
                            const title = m[1].trim()
                            if (!title) continue
                            const target = notes.value.find(t => t.title === title) || notes.value.find(t => t.title.toLowerCase() === title.toLowerCase())
                            if (target) link(n, ensure('note', String(target.id), target.title))
                        }
                        const mAddr = /\{\{([^}|]+)/g
                        while ((m = mAddr.exec(content))) {
                            const name = m[1].trim()
                            if (!name) continue
                            const target = contacts.value.find(c => String(c.name || '').trim() === name) || contacts.value.find(c => String(c.name || '').trim().toLowerCase() === name.toLowerCase())
                            if (target) link(n, ensure('contact', String(target.id), target.name))
                        }
                        const mWiki = /<<([^>|]+)/g
                        while ((m = mWiki.exec(content))) {
                            const title = m[1].trim()
                            if (!title) continue
                            const target = wikiPages.value.find(p => String(p.title || '').trim() === title) || wikiPages.value.find(p => String(p.title || '').trim().toLowerCase() === title.toLowerCase())
                            if (target) link(n, ensure('wiki', String(target.id), target.title))
                        }
                    })
                    contacts.value.forEach(c => {
                        const cn = ensure('contact', String(c.id), c.name || t('graph.contact'))
                        getContactDepartments(c).forEach(d => link(cn, ensure('dept', d, d)))
                    })
                    wikiPages.value.forEach(p => ensure('wiki', String(p.id), p.title || 'Wiki-Seite'))
                    graphNodeData.value = nodes
                    graphLinkData.value = links
                    const counts = { note: 0, contact: 0, wiki: 0, dept: 0 }
                    nodes.forEach(n => { counts[n.type]++ })
                    graphCounts.value = counts
                    nodes.forEach(n => { graphPositions[n.id] = { x: n.x, y: n.y } })
                }

                const graphStep = () => {
                    const nodes = graphNodeData.value
                    if (!nodes.length) return
                    const links = graphLinkData.value
                    const vis = graphTypeVisible.value
                    for (let i = 0; i < nodes.length; i++) {
                        const a = nodes[i]
                        if (!vis[a.type]) continue
                        a.fx = 0
                        a.fy = 0
                        for (let j = i + 1; j < nodes.length; j++) {
                            const b = nodes[j]
                            if (!vis[b.type]) continue
                            const dx = a.x - b.x
                            const dy = a.y - b.y
                            const d2 = dx * dx + dy * dy
                            if (d2 < 0.01) continue
                            const d = Math.sqrt(d2)
                            let f = 900 / d2
                            if (f > 30) f = 30
                            const fx = f * dx / d
                            const fy = f * dy / d
                            a.fx += fx
                            a.fy += fy
                            b.fx -= fx
                            b.fy -= fy
                        }
                    }
                    const ideal = 110
                    links.forEach(l => {
                        if (!vis[l.a.type] || !vis[l.b.type]) return
                        const dx = l.b.x - l.a.x
                        const dy = l.b.y - l.a.y
                        const d = Math.sqrt(dx * dx + dy * dy) || 1
                        const f = (d - ideal) * 0.012
                        const kx = f * dx / d
                        const ky = f * dy / d
                        l.a.fx += kx
                        l.a.fy += ky
                        l.b.fx -= kx
                        l.b.fy -= ky
                    })
                    nodes.forEach(n => {
                        if (!vis[n.type]) return
                        n.fx += -n.x * 0.0015
                        n.fy += -n.y * 0.0015
                        n.vx = (n.vx + n.fx) * 0.90
                        n.vy = (n.vy + n.fy) * 0.90
                        if (n.vx > 4) n.vx = 4
                        else if (n.vx < -4) n.vx = -4
                        if (n.vy > 4) n.vy = 4
                        else if (n.vy < -4) n.vy = -4
                        if (n === graphDragNode) {
                            n.vx = 0
                            n.vy = 0
                        }
                        n.x += n.vx
                        n.y += n.vy
                    })
                }

                const graphWarm = () => {
                    for (let i = 0; i < 300; i++) graphStep()
                }

                const graphNodeRadius = (type) => (type === 'dept' ? 10 : 7)

                const graphDraw = () => {
                    const canvas = graphCanvasRef.value
                    const box = graphContainerRef.value
                    if (!canvas || !box) return
                    const rect = box.getBoundingClientRect()
                    const dpr = window.devicePixelRatio || 1
                    const w = Math.max(1, Math.round(rect.width))
                    const h = Math.max(1, Math.round(rect.height))
                    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
                        canvas.width = Math.round(w * dpr)
                        canvas.height = Math.round(h * dpr)
                        canvas.style.width = w + 'px'
                        canvas.style.height = h + 'px'
                    }
                    const ctx = canvas.getContext('2d')
                    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
                    ctx.clearRect(0, 0, w, h)
                    const vis = graphTypeVisible.value
                    const hoverId = graphHoverId.value
                    const selId = graphSelectedId.value
                    const keyId = hoverId || selId || null
                    const highlightIds = new Set()
                    if (keyId) {
                        graphLinkData.value.forEach(l => {
                            if (l.a.id === keyId || l.b.id === keyId) {
                                highlightIds.add(l.a.id)
                                highlightIds.add(l.b.id)
                            }
                        })
                    }
                    ctx.save()
                    ctx.translate(w / 2 + graphCam.x, h / 2 + graphCam.y)
                    ctx.scale(graphCam.zoom, graphCam.zoom)
                    graphLinkData.value.forEach(l => {
                        if (!vis[l.a.type] || !vis[l.b.type]) return
                        const hl = highlightIds.has(l.a.id) || highlightIds.has(l.b.id)
                        ctx.strokeStyle = hl ? graphTypeColor(l.a.type) : 'rgba(120,120,130,0.35)'
                        ctx.globalAlpha = hl ? 0.9 : 0.6
                        ctx.lineWidth = (hl ? 2 : 1) / graphCam.zoom
                        ctx.beginPath()
                        ctx.moveTo(l.a.x, l.a.y)
                        ctx.lineTo(l.b.x, l.b.y)
                        ctx.stroke()
                    })
                    ctx.globalAlpha = 1
                    graphNodeData.value.forEach(n => {
                        if (!vis[n.type]) return
                        const r = graphNodeRadius(n.type)
                        const col = graphTypeColor(n.type)
                        const hl = (n.id === hoverId || n.id === selId)
                        ctx.beginPath()
                        ctx.arc(n.x, n.y, r, 0, Math.PI * 2)
                        ctx.fillStyle = col
                        ctx.globalAlpha = hoverId && !highlightIds.has(n.id) ? 0.35 : 1
                        ctx.fill()
                        ctx.globalAlpha = 1
                        ctx.lineWidth = (hl ? 2.5 : 1) / graphCam.zoom
                        ctx.strokeStyle = hl ? '#fafafa' : 'rgba(0,0,0,0.35)'
                        ctx.stroke()
                        ctx.font = '10px ui-monospace, SFMono-Regular, Menlo, monospace'
                        ctx.fillStyle = hl ? col : 'rgba(140,140,150,0.9)'
                        ctx.fillText(n.label.length > 26 ? n.label.slice(0, 25) + '…' : n.label, n.x + r + 5, n.y + 3)
                    })
                    ctx.restore()
                }

                const graphStart = () => {
                    if (graphRAF) return
                    graphRAF = requestAnimationFrame(function graphLoop() {
                        graphStep()
                        graphDraw()
                        graphRAF = requestAnimationFrame(graphLoop)
                    })
                }

                const graphStop = () => {
                    if (graphRAF) {
                        cancelAnimationFrame(graphRAF)
                        graphRAF = null
                    }
                    graphDragNode = null
                    graphPanning = false
                }

                const graphWorldPos = (clientX, clientY) => {
                    const rect = graphContainerRef.value.getBoundingClientRect()
                    const px = clientX - rect.left
                    const py = clientY - rect.top
                    return {
                        x: (px - rect.width / 2 - graphCam.x) / graphCam.zoom,
                        y: (py - rect.height / 2 - graphCam.y) / graphCam.zoom
                    }
                }

                const graphNodeAt = (clientX, clientY) => {
                    const w = graphWorldPos(clientX, clientY)
                    const vis = graphTypeVisible.value
                    const nodes = graphNodeData.value
                    for (let i = nodes.length - 1; i >= 0; i--) {
                        const n = nodes[i]
                        if (!vis[n.type]) continue
                        const dx = n.x - w.x
                        const dy = n.y - w.y
                        const rr = graphNodeRadius(n.type) + 6 / graphCam.zoom
                        if (dx * dx + dy * dy <= rr * rr) return n
                    }
                    return null
                }

                const graphMouseDown = (e) => {
                    if (e.button !== 0) return
                    const n = graphNodeAt(e.clientX, e.clientY)
                    if (n) {
                        e.preventDefault()
                        graphDragNode = n
                        graphDraggedDist = 0
                        const w = graphWorldPos(e.clientX, e.clientY)
                        n.x = w.x
                        n.y = w.y
                        n.vx = 0
                        n.vy = 0
                        graphSelectedId.value = n.id
                        graphHoverId.value = n.id
                    } else {
                        graphPanning = true
                        graphPanStart = { x: e.clientX - graphCam.x, y: e.clientY - graphCam.y }
                        graphSelectedId.value = null
                    }
                }

                const graphMouseMove = (e) => {
                    if (graphDragNode) {
                        const w = graphWorldPos(e.clientX, e.clientY)
                        graphDragNode.x = w.x
                        graphDragNode.y = w.y
                        graphDraggedDist += Math.abs(e.movementX || 0) + Math.abs(e.movementY || 0)
                        return
                    }
                    if (graphPanning && graphPanStart) {
                        graphCam.x = e.clientX - graphPanStart.x
                        graphCam.y = e.clientY - graphPanStart.y
                        return
                    }
                    const n = graphNodeAt(e.clientX, e.clientY)
                    graphHoverId.value = n ? n.id : null
                    graphContainerRef.value.style.cursor = n ? 'pointer' : 'grab'
                }

                const graphMouseUp = () => {
                    if (graphDragNode && graphDraggedDist > 5) graphSelectedId.value = graphDragNode.id
                    graphDragNode = null
                    graphPanning = false
                    graphPanStart = null
                    graphNormalizeCursor()
                }

                const graphMouseLeave = () => {
                    if (!graphDragNode && !graphPanning) graphHoverId.value = null
                }

                const graphWheel = (e) => {
                    const rect = graphContainerRef.value.getBoundingClientRect()
                    const cx = e.clientX - rect.left
                    const cy = e.clientY - rect.top
                    const factor = Math.pow(1.1, -e.deltaY / 100)
                    const wx = (cx - rect.width / 2 - graphCam.x) / graphCam.zoom
                    const wy = (cy - rect.height / 2 - graphCam.y) / graphCam.zoom
                    graphCam.zoom = Math.min(3, Math.max(0.15, graphCam.zoom * factor))
                    graphCam.x = cx - rect.width / 2 - wx * graphCam.zoom
                    graphCam.y = cy - rect.height / 2 - wy * graphCam.zoom
                }

                const graphOpenNode = (n) => {
                    if (!n || !n.id) return
                    if (n.type === 'note') {
                        const note = notes.value.find(x => x.id == n.key)
                        if (note) openModal(note)
                    } else if (n.type === 'contact') {
                        const c = contacts.value.find(x => x.id == n.key)
                        if (c) openContactModal(c)
                    } else if (n.type === 'wiki') {
                        const p = wikiPages.value.find(x => x.id == n.key)
                        if (p) {
                            switchView('wiki')
                            openWikiPage(p)
                        }
                    }
                }

                const graphDblClick = (e) => {
                    const n = graphNodeAt(e.clientX, e.clientY)
                    if (n) {
                        graphSelectedId.value = n.id
                        graphOpenNode(n)
                    }
                }

                const graphSelectedInfo = computed(() => {
                    const nd = graphSelected.value
                    if (!nd) return null
                    const props = []
                    if (nd.type === 'note') {
                        const note = notes.value.find(n => n.id == nd.key)
                        if (note) {
                            props.push([t('note.status'), statusTitle(note.status)])
                            if (note.priority) props.push([t('note.priority'), t('priority.' + note.priority)])
                            if (note.due_date) props.push([t('note.due'), fmtDate(note.due_date)])
                            if (note.repeat_rule) props.push([t('note.repeat'), t('repeat.' + note.repeat_rule)])
                            const depts = getDepartments(note)
                            if (depts.length) props.push([t('common.departments'), depts.join(', ')])
                            const apts = getAppointments(note).filter(a => !a.done)
                            if (apts.length) props.push([t('common.appointments'), apts.length])
                        }
                    } else if (nd.type === 'contact') {
                        const c = contacts.value.find(cc => cc.id == nd.key)
                        if (c) {
                            if (c.phone) props.push([t('common.phone'), c.phone])
                            if (c.email) props.push([t('common.email'), c.email])
                            const depts = getContactDepartments(c)
                            if (depts.length) props.push([t('common.departments'), depts.join(', ')])
                        }
                    } else if (nd.type === 'wiki') {
                        const p = wikiPages.value.find(pp => pp.id == nd.key)
                        if (p && p.updated_at) props.push([t('graph.updated'), fmtWikiDate(p.updated_at)])
                    } else if (nd.type === 'dept') {
                        const connected = new Set()
                        const noteSet = new Set()
                        const contactSet = new Set()
                        graphLinkData.value.forEach(l => {
                            if (l.a.id !== nd.id && l.b.id !== nd.id) return
                            const other = l.a.id === nd.id ? l.b : l.a
                            connected.add(other.id)
                            if (other.type === 'note') noteSet.add(other.id)
                            if (other.type === 'contact') contactSet.add(other.id)
                        })
                        props.push([t('graph.links'), connected.size])
                        props.push([t('common.notes'), noteSet.size])
                        props.push([t('data.contacts'), contactSet.size])
                    }
                    const groupMap = {}
                    graphLinkData.value.forEach(l => {
                        if (l.a.id !== nd.id && l.b.id !== nd.id) return
                        const other = l.a.id === nd.id ? l.b : l.a
                        if (!graphTypeVisible.value[other.type]) return
                        const list = groupMap[other.type] || (groupMap[other.type] = [])
                        if (!list.some(x => x.id === other.id)) list.push(other)
                    })
                    const neighbors = Object.keys(groupMap)
                        .map(type => ({ type, list: groupMap[type] }))
                        .sort((a, b) => graphTypes.findIndex(t => t.id === a.type) - graphTypes.findIndex(t => t.id === b.type))
                    return { id: nd.id, type: nd.type, label: nd.label, color: graphTypeColor(nd.type), props, neighbors }
                })

                const graphNormalizeCursor = () => {
                    if (graphContainerRef.value) graphContainerRef.value.style.cursor = 'grab'
                }

                watch(activeView, (v) => {
                    if (v === 'graph') {
                        rebuildGraph()
                        graphWarm()
                        graphCam.x = 0
                        graphCam.y = 0
                        graphCam.zoom = 1
                        graphSelectedId.value = null
                        graphNormalizeCursor()
                        graphStart()
                    } else {
                        graphStop()
                    }
                })

                watch([notes, contacts, wikiPages], () => {
                    if (activeView.value === 'graph') {
                        rebuildGraph()
                        graphWarm()
                    }
                })

                return {
                    activeView,
                    statusTitle,
                    dowShorts,
                    language,
                    t,
                    locale,
                    switchLanguage,
                    saveConfig,
                    applyConfig,
                    isDark,
                    toggleTheme,
                    isSettingsOpen,
                    shortcutHelpOpen,
                    draftTs,
                    draftRestored,
                    fmtClock,
                    clearNewNoteDraft,
                    autoArchiveEnabled,
                    autoArchiveDay,
                    trashPurgeEnabled,
                    trashPurgeDay,
                    networkOpen,
                    toggleNetworkOpen,
                    runTrashPurgeNow,
                    purgeTrash,
                    pendingImport,
                    existingCount,
                    existingContactCount,
                    cancelImport,
                    confirmImport,
                    undoSlot,
                    undoActive,
                    performUndo,
                    reminderOpen,
                    overdueReminders,
                    openNoteFromReminder,
                    completeReminderAppointment,
                    timerOpen,
                    timerTotal,
                    timerRemaining,
                    timerRunning,
                    fmtTimer,
                    timerSet,
                    timerToggle,
                    timerReset,
                    newNoteTitle,
                    newNotePriority,
                    newNoteDueDate,
                    newNoteRepeatRule,
                    newNoteStatus,
                    newNoteContent,
                    newNoteChecklist,
                    newNoteStepText,
                    newNoteDepartment,
                    newNoteDepartments,
                    newNoteDeptInput,
                    newNoteDepartmentInputRef,
                    modalDepartmentInputRef,
                    newNoteAppointments,
                    newAptTitle,
                    newAptStart,
                    newAptEnd,
                    newAptTime,
                    newAptEndTime,
                    newAptHasEnd,
                    addNewAppointment,
                    addModalAppointment,
                    activeNoteAppointments,
                    activeNoteDepartments,
                    modalDeptInput,
                    modalAptTitle,
                    modalAptStart,
                    modalAptEnd,
                    modalAptTime,
                    modalAptEndTime,
                    modalAptHasEnd,
                    removeNewNoteDepartment,
                    removeActiveNoteDepartment,
                    addCustomDepartment,
                    getDepartments,
                    getAppointments,
                    getChecklistProgress,
                    isNewNoteOpen,
                    newNoteTitleInputRef,
                    newNoteTextareaRef,
                    newNoteCaretMirrorRef,
                    newNoteStepInputRef,
                    wikiTextareaRef,
                    wikiCaretMirrorRef,
                    confirmOpen,
                    confirmMessage,
                    confirmOkLabel,
                    confirmAccept,
                    confirmCancel,
                    columns,
                    notes,
                    notesById,
                    isModalOpen,
                    isPreviewMode,
                    toggleDetailPreview,
                    detailMarkdownMax,
                    newNotePreviewMode,
                    newNoteMarkdownMax,
                    toggleNewNotePreview,
                    newNoteRenderedMarkdown,
                    onNewNotePreviewClick,
                    onEditorPaste,
                    triggerImageUpload,
                    onImageFileSelected,
                    imageUploadInputRef,
                    activeNote,
                    activeChecklist,
                    newStepText,
                    isBacklogCollapsed,
                    isArchiveCollapsed,
                    collapsedStatuses,
                    expandedColumns,
                    isCollapsed,
                    toggleCollapse,
                    expandColumn,
                    collapseColumn,
                    gridColsStyle,
                    mobileBoardColumn,
                    wheelMenuOpen,
                    wheelCenter,
                    wheelStart,
                    wheelCancel,
                    wheelSelect,
                    wheelButtonStyle,
                    mobileCardClick,
                    mobileDraggedNote,
                    mobileDragInsertY,
                    onMobileTouchMove,
                    onMobileTouchEnd,
                    searchQuery,
                    searchInputRef,
                    textareaRef,
                    caretMirrorRef,
                    isOverdue,
                    isDueSoon,
                    renderedMarkdown,
                    onPreviewClick,
                    contextMenu,
                    exportMenuOpen,
                    mobileMenuOpen,
                    openContextMenu,
                    closeContextMenu,
                    applyFormat,
                    applyHtmlColor,
                    openNewNote,
                    closeNewNote,
                    createNote,
                    addNewNoteStep,
                    removeNewNoteStep,
                    handleNewNoteEnter,
                    cancelAutocomplete,
                    exportJson,
                    exportCsv,
                    exportFullBackup,
                    startImport,
                    onImportFile,
                    importFileInputRef,
                    trashNotes,
                    trashContacts,
                    fetchTrash,
                    restoreNote,
                    restoreContact,
                    forceDeleteNote,
                    forceDeleteContact,
                    clearTrash,
                    archivedNotes,
                    fetchArchivedNotes,
                    restoreFromArchive,
                    hardDeleteArchived,
                    openModal,
                    closeModal,
                    addStep,
                    removeStep,
                    stepDragStart,
                    stepDrop,
                    saveActiveNote,
                    deleteNote,
                    duplicateNote,
                    archiveNote,
                    unarchiveNote,
                    getNotesByColumn,
                    startDrag,
                    endDrag,
                    dragCol,
                    dragInsertY,
                    onColDragOver,
                    onColDrop,
                    boardVisibility,
                    boardDept,
                    handleAutocomplete,
                    handleAutocompleteKeydown,
                    selectAutocomplete,
                    showAutocomplete,
                    autocompleteResults,
                    autocompleteIndex,
                    autocompletePos,
                    updateAutocompletePos,
                    handleNoteLinkClick,
                    focusSearchInput,
                    processNoteLinks,
                    departments,
                    filteredDepartments,
                    deptDropdownOpen,
                    deptDropdownSource,
                    deptIndex,
                    openDeptDropdown,
                    closeDeptDropdown,
                    selectDepartment,
                    onDeptInput,
                    handleDeptKeydown,
                    calViewMode,
                    calCursor,
                    calSelectedDay,
                    calMonthDays,
                    calWeekDays,
                    calYearMonths,
                    calHours,
                    calTitle,
                    calToday,
                    calNavigate,
                    calDayClick,
                    calSelectedDayTitle,
                    calSelectedDayEvents,
                    calWeekScrollRef,
                    scrollWeekToNow,
                    calWeekDayIndex,
                    calWeekShift,
                    calEventDragStart,
                    calDayDrop,
                    fmtDate,
                    fmtAptRange,
                    dateStr,
                    convertDateToISO,
                    processAddressLinks,
                    handleAddressLinkClick,
                    contactPopover,
                    showContactPopover,
                    hideContactPopover,
                    contacts,
                    contactFilter,
                    contactModalOpen,
                    contactForm,
                    highlightContactId,
                    filteredContacts,
                    fetchContacts,
                    openContactModal,
                    saveContact,
                    getContactDepartments,
                    contactDeptInput,
                    contactDepartmentInputRef,
                    removeContactDepartment,
                    newDeptName,
                    deptError,
                    createDepartment,
                    wikiPages,
                    wikiFilter,
                    wikiCurrent,
                    wikiForm,
                    wikiPreviewOpen,
                    wikiStatus,
                    wikiStatusOk,
                    filteredWikiPages,
                    wikiPreviewHtml,
                    fmtWikiDate,
                    fetchWikiPages,
                    openWikiPage,
                    newWikiPage,
                    saveWikiPage,
                    deleteWikiPage,
                    switchView,
                    deleteContact,
                    deleteDepartment,
                    dashboardTodayEvents,
                    overdueNotes,
                    dashboardUpcoming,
                    dashboardStatusRows,
                    dashboardPriorityRows,
                    dashboardActivity,
                    dashboardActivityDays,
                    dashboardTodo,
                    dashboardDeptRows,
                    graphTypes,
                    graphTypeColor,
                    graphTypeLabel,
                    graphTypeVisible,
                    graphNodeData,
                    graphLinkData,
                    graphCounts,
                    graphHoverId,
                    graphSelectedId,
                    graphSelected,
                    graphSelectedInfo,
                    graphCanvasRef,
                    graphContainerRef,
                    graphHint,
                    graphMouseDown,
                    graphMouseMove,
                    graphMouseUp,
                    graphMouseLeave,
                    graphWheel,
                    graphDblClick,
                    graphOpenNode,
                    rebuildGraph
                }
            }
        }).mount('#app')