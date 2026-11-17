import { createRouter, createWebHistory } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import WorkspaceLayout from '@/layouts/WorkspaceLayout.vue'

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/setup',
      name: 'setup',
      component: () => import('@/views/SetupView.vue'),
    },
    {
      path: '/login',
      name: 'login',
      component: () => import('@/views/LoginView.vue'),
    },
    {
      path: '/',
      component: WorkspaceLayout,
      children: [
        {
          path: '',
          name: 'dashboard',
          component: () => import('@/views/DashboardView.vue'),
        },
        {
          path: 'books',
          name: 'books',
          component: () => import('@/views/books/BooksHubView.vue'),
        },
        {
          path: 'books/read/:id',
          name: 'book-reader',
          component: () => import('@/views/books/BookReaderView.vue'),
        },
        {
          path: 'comics',
          name: 'comics',
          component: () => import('@/views/comics/ComicsHubView.vue'),
        },
        {
          path: 'comics/series/:id',
          name: 'comic-series',
          component: () => import('@/views/comics/ComicSeriesView.vue'),
        },
        {
          path: 'comics/read/:chapterId',
          name: 'comic-reader',
          component: () => import('@/views/comics/ComicReaderView.vue'),
        },
        {
          path: 'music',
          name: 'music',
          component: () => import('@/views/music/MusicHubView.vue'),
        },
        {
          path: 'videos',
          name: 'videos',
          component: () => import('@/views/videos/VideosHubView.vue'),
        },
        {
          path: 'videos/shows/:id',
          name: 'video-show',
          component: () => import('@/views/videos/VideoShowView.vue'),
        },
        {
          path: 'videos/watch/:id',
          name: 'video-player',
          component: () => import('@/views/videos/VideoPlayerView.vue'),
        },
        {
          path: 'settings',
          name: 'settings',
          component: () => import('@/views/SettingsView.vue'),
        },
      ],
    },
  ],
})

router.beforeEach(async (to) => {
  const auth = useAuthStore()
  await auth.bootstrap()

  if (auth.needsSetup) {
    if (to.name !== 'setup') return { name: 'setup' }
    return true
  }
  if (to.name === 'setup') {
    return auth.isAuthenticated ? { name: 'dashboard' } : { name: 'login' }
  }
  if (!auth.isAuthenticated && to.name !== 'login') {
    return { name: 'login' }
  }
  if (auth.isAuthenticated && to.name === 'login') {
    return { name: 'dashboard' }
  }
  return true
})
