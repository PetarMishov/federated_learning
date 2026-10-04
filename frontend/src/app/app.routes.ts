import { inject } from '@angular/core';
import { CanActivateFn, Router, Routes } from '@angular/router';
import { Home } from './home/home';
import { Layout } from './layout/layout';
import { Login } from './login/login';
import { UsersApi } from './users-api';

const authenticated: CanActivateFn = () =>
  inject(UsersApi).token() ? true : inject(Router).createUrlTree(['/login']);

export const routes: Routes = [
  { path: '', pathMatch: 'full', redirectTo: 'login' },
  { path: 'login', component: Login },
  {
    path: '',
    component: Layout,
    canActivate: [authenticated],
    canActivateChild: [authenticated],
    children: [{ path: 'home', component: Home }],
  },
  { path: '**', redirectTo: 'login' },
];
