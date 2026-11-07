
/var/tmp/code-263-profile-before/threshold-full:     file format elf64-x86-64


Disassembly of section .init:

Disassembly of section .plt:

Disassembly of section .plt.got:

Disassembly of section .text:

000000000000ddd0 <code_add_into>:
    ddd0:	41 57                	push   %r15
    ddd2:	49 89 ff             	mov    %rdi,%r15
    ddd5:	41 56                	push   %r14
    ddd7:	49 89 f6             	mov    %rsi,%r14
    ddda:	41 55                	push   %r13
    dddc:	41 54                	push   %r12
    ddde:	55                   	push   %rbp
    dddf:	53                   	push   %rbx
    dde0:	48 81 ec b8 00 00 00 	sub    $0xb8,%rsp
    dde7:	48 39 fe             	cmp    %rdi,%rsi
    ddea:	74 1e                	je     de0a <code_add_into+0x3a>
    ddec:	8b 07                	mov    (%rdi),%eax
    ddee:	83 f8 04             	cmp    $0x4,%eax
    ddf1:	0f 84 d1 00 00 00    	je     dec8 <code_add_into+0xf8>
    ddf7:	83 f8 01             	cmp    $0x1,%eax
    ddfa:	74 34                	je     de30 <code_add_into+0x60>
    ddfc:	83 f8 05             	cmp    $0x5,%eax
    ddff:	75 09                	jne    de0a <code_add_into+0x3a>
    de01:	83 3e 05             	cmpl   $0x5,(%rsi)
    de04:	0f 84 46 02 00 00    	je     e050 <code_add_into+0x280>
    de0a:	48 81 c4 b8 00 00 00 	add    $0xb8,%rsp
    de11:	4c 89 f2             	mov    %r14,%rdx
    de14:	4c 89 fe             	mov    %r15,%rsi
    de17:	4c 89 ff             	mov    %r15,%rdi
    de1a:	5b                   	pop    %rbx
    de1b:	5d                   	pop    %rbp
    de1c:	41 5c                	pop    %r12
    de1e:	41 5d                	pop    %r13
    de20:	41 5e                	pop    %r14
    de22:	41 5f                	pop    %r15
    de24:	e9 37 f3 ff ff       	jmp    d160 <code_add>
    de29:	0f 1f 80 00 00 00 00 	nopl   0x0(%rax)
    de30:	83 3e 01             	cmpl   $0x1,(%rsi)
    de33:	75 d5                	jne    de0a <code_add_into+0x3a>
    de35:	8b 77 04             	mov    0x4(%rdi),%esi
    de38:	85 f6                	test   %esi,%esi
    de3a:	74 0b                	je     de47 <code_add_into+0x77>
    de3c:	48 8b 47 08          	mov    0x8(%rdi),%rax
    de40:	48 83 78 f0 01       	cmpq   $0x1,-0x10(%rax)
    de45:	75 c3                	jne    de0a <code_add_into+0x3a>
    de47:	48 8d 5c 24 30       	lea    0x30(%rsp),%rbx
    de4c:	4c 89 fe             	mov    %r15,%rsi
    de4f:	48 89 df             	mov    %rbx,%rdi
    de52:	e8 f9 66 ff ff       	call   4550 <append_buffer>
    de57:	4c 89 f7             	mov    %r14,%rdi
    de5a:	e8 c1 52 ff ff       	call   3120 <text_length>
    de5f:	49 8b 76 08          	mov    0x8(%r14),%rsi
    de63:	48 89 df             	mov    %rbx,%rdi
    de66:	48 89 c2             	mov    %rax,%rdx
    de69:	e8 a2 71 ff ff       	call   5010 <text_push>
    de6e:	31 d2                	xor    %edx,%edx
    de70:	48 8d 35 e2 24 00 00 	lea    0x24e2(%rip),%rsi        # 10359 <_fini+0x539>
    de77:	48 89 df             	mov    %rbx,%rdi
    de7a:	e8 91 71 ff ff       	call   5010 <text_push>
    de7f:	48 8b 4c 24 30       	mov    0x30(%rsp),%rcx
    de84:	48 8b 54 24 38       	mov    0x38(%rsp),%rdx
    de89:	48 8b 44 24 40       	mov    0x40(%rsp),%rax
    de8e:	c6 04 11 00          	movb   $0x0,(%rcx,%rdx,1)
    de92:	48 85 c0             	test   %rax,%rax
    de95:	74 0b                	je     dea2 <code_add_into+0xd2>
    de97:	48 0f bd c0          	bsr    %rax,%rax
    de9b:	83 c0 01             	add    $0x1,%eax
    de9e:	48 c1 e0 3a          	shl    $0x3a,%rax
    dea2:	48 83 c2 01          	add    $0x1,%rdx
    dea6:	48 09 c2             	or     %rax,%rdx
    dea9:	48 b8 01 00 00 00 01 	movabs $0x100000001,%rax
    deb0:	00 00 00 
    deb3:	48 89 51 f8          	mov    %rdx,-0x8(%rcx)
    deb7:	49 89 07             	mov    %rax,(%r15)
    deba:	49 89 4f 08          	mov    %rcx,0x8(%r15)
    debe:	e9 72 01 00 00       	jmp    e035 <code_add_into+0x265>
    dec3:	0f 1f 44 00 00       	nopl   0x0(%rax,%rax,1)
    dec8:	8b 47 04             	mov    0x4(%rdi),%eax
    decb:	85 c0                	test   %eax,%eax
    decd:	0f 85 7d 00 00 00    	jne    df50 <code_add_into+0x180>
    ded3:	83 3e 04             	cmpl   $0x4,(%rsi)
    ded6:	0f 84 94 02 00 00    	je     e170 <code_add_into+0x3a0>
    dedc:	48 8b 47 08          	mov    0x8(%rdi),%rax
    dee0:	48 85 c0             	test   %rax,%rax
    dee3:	0f 84 67 02 00 00    	je     e150 <code_add_into+0x380>
    dee9:	48 8b 18             	mov    (%rax),%rbx
    deec:	bd 01 00 00 00       	mov    $0x1,%ebp
    def1:	4c 8d 63 01          	lea    0x1(%rbx),%r12
    def5:	b8 04 00 00 00       	mov    $0x4,%eax
    defa:	49 39 c4             	cmp    %rax,%r12
    defd:	49 0f 4d c4          	cmovge %r12,%rax
    df01:	48 89 c7             	mov    %rax,%rdi
    df04:	48 89 c2             	mov    %rax,%rdx
    df07:	48 c1 e7 04          	shl    $0x4,%rdi
    df0b:	48 83 c7 20          	add    $0x20,%rdi
    df0f:	48 89 14 24          	mov    %rdx,(%rsp)
    df13:	e8 c8 42 ff ff       	call   21e0 <malloc@plt>
    df18:	48 85 c0             	test   %rax,%rax
    df1b:	0f 84 58 07 00 00    	je     e679 <code_add_into+0x8a9>
    df21:	66 0f 6f 05 17 31 00 	movdqa 0x3117(%rip),%xmm0        # 11040 <no_value+0x7c0>
    df28:	00 
    df29:	0f 11 00             	movups %xmm0,(%rax)
    df2c:	f0 48 83 05 f3 f1 00 	lock addq $0x1,0xf1f3(%rip)        # 1d128 <live_blocks>
    df33:	00 01 
    df35:	48 8b 14 24          	mov    (%rsp),%rdx
    df39:	48 c7 40 18 00 00 00 	movq   $0x0,0x18(%rax)
    df40:	00 
    df41:	4c 8d 68 10          	lea    0x10(%rax),%r13
    df45:	48 89 50 10          	mov    %rdx,0x10(%rax)
    df49:	eb 72                	jmp    dfbd <code_add_into+0x1ed>
    df4b:	0f 1f 44 00 00       	nopl   0x0(%rax,%rax,1)
    df50:	4c 8b 6f 08          	mov    0x8(%rdi),%r13
    df54:	49 8b 6d f0          	mov    -0x10(%r13),%rbp
    df58:	48 83 fd 01          	cmp    $0x1,%rbp
    df5c:	0f 85 a8 fe ff ff    	jne    de0a <code_add_into+0x3a>
    df62:	83 3e 04             	cmpl   $0x4,(%rsi)
    df65:	0f 84 05 02 00 00    	je     e170 <code_add_into+0x3a0>
    df6b:	49 8b 5d 00          	mov    0x0(%r13),%rbx
    df6f:	4c 8d 63 01          	lea    0x1(%rbx),%r12
    df73:	4c 89 e0             	mov    %r12,%rax
    df76:	49 8d 7d f0          	lea    -0x10(%r13),%rdi
    df7a:	48 c1 e0 04          	shl    $0x4,%rax
    df7e:	48 89 7c 24 08       	mov    %rdi,0x8(%rsp)
    df83:	48 8d 50 20          	lea    0x20(%rax),%rdx
    df87:	48 89 14 24          	mov    %rdx,(%rsp)
    df8b:	e8 90 41 ff ff       	call   2120 <malloc_usable_size@plt>
    df90:	48 8b 14 24          	mov    (%rsp),%rdx
    df94:	48 39 d0             	cmp    %rdx,%rax
    df97:	73 24                	jae    dfbd <code_add_into+0x1ed>
    df99:	48 01 c0             	add    %rax,%rax
    df9c:	48 8b 7c 24 08       	mov    0x8(%rsp),%rdi
    dfa1:	48 39 d0             	cmp    %rdx,%rax
    dfa4:	48 0f 43 d0          	cmovae %rax,%rdx
    dfa8:	48 89 d6             	mov    %rdx,%rsi
    dfab:	e8 40 42 ff ff       	call   21f0 <realloc@plt>
    dfb0:	48 85 c0             	test   %rax,%rax
    dfb3:	0f 84 c0 06 00 00    	je     e679 <code_add_into+0x8a9>
    dfb9:	4c 8d 68 10          	lea    0x10(%rax),%r13
    dfbd:	48 85 ed             	test   %rbp,%rbp
    dfc0:	7e 63                	jle    e025 <code_add_into+0x255>
    dfc2:	48 c1 e3 04          	shl    $0x4,%rbx
    dfc6:	31 d2                	xor    %edx,%edx
    dfc8:	31 f6                	xor    %esi,%esi
    dfca:	4d 8d 4c 1d 00       	lea    0x0(%r13,%rbx,1),%r9
    dfcf:	90                   	nop
    dfd0:	41 83 3e 04          	cmpl   $0x4,(%r14)
    dfd4:	4c 89 f0             	mov    %r14,%rax
    dfd7:	75 12                	jne    dfeb <code_add_into+0x21b>
    dfd9:	49 8b 46 08          	mov    0x8(%r14),%rax
    dfdd:	48 85 c0             	test   %rax,%rax
    dfe0:	48 8d 78 10          	lea    0x10(%rax),%rdi
    dfe4:	48 0f 45 c7          	cmovne %rdi,%rax
    dfe8:	48 01 d0             	add    %rdx,%rax
    dfeb:	8b 78 04             	mov    0x4(%rax),%edi
    dfee:	85 ff                	test   %edi,%edi
    dff0:	74 1c                	je     e00e <code_add_into+0x23e>
    dff2:	8b 38                	mov    (%rax),%edi
    dff4:	83 ff 01             	cmp    $0x1,%edi
    dff7:	74 0c                	je     e005 <code_add_into+0x235>
    dff9:	83 ef 04             	sub    $0x4,%edi
    dffc:	83 ff 01             	cmp    $0x1,%edi
    dfff:	0f 87 25 45 ff ff    	ja     252a <code_add_into.cold>
    e005:	48 8b 78 08          	mov    0x8(%rax),%rdi
    e009:	48 83 47 f0 01       	addq   $0x1,-0x10(%rdi)
    e00e:	f3 0f 6f 00          	movdqu (%rax),%xmm0
    e012:	48 83 c6 01          	add    $0x1,%rsi
    e016:	41 0f 11 44 11 10    	movups %xmm0,0x10(%r9,%rdx,1)
    e01c:	48 83 c2 10          	add    $0x10,%rdx
    e020:	48 39 f5             	cmp    %rsi,%rbp
    e023:	75 ab                	jne    dfd0 <code_add_into+0x200>
    e025:	4d 89 65 00          	mov    %r12,0x0(%r13)
    e029:	4d 89 6f 08          	mov    %r13,0x8(%r15)
    e02d:	41 c7 47 04 01 00 00 	movl   $0x1,0x4(%r15)
    e034:	00 
    e035:	48 81 c4 b8 00 00 00 	add    $0xb8,%rsp
    e03c:	5b                   	pop    %rbx
    e03d:	5d                   	pop    %rbp
    e03e:	41 5c                	pop    %r12
    e040:	41 5d                	pop    %r13
    e042:	41 5e                	pop    %r14
    e044:	41 5f                	pop    %r15
    e046:	c3                   	ret
    e047:	66 0f 1f 84 00 00 00 	nopw   0x0(%rax,%rax,1)
    e04e:	00 00 
    e050:	8b 4f 04             	mov    0x4(%rdi),%ecx
    e053:	85 c9                	test   %ecx,%ecx
    e055:	0f 84 af fd ff ff    	je     de0a <code_add_into+0x3a>
    e05b:	48 8b 47 08          	mov    0x8(%rdi),%rax
    e05f:	48 83 78 f0 01       	cmpq   $0x1,-0x10(%rax)
    e064:	0f 85 a0 fd ff ff    	jne    de0a <code_add_into+0x3a>
    e06a:	48 8b 5e 08          	mov    0x8(%rsi),%rbx
    e06e:	48 85 db             	test   %rbx,%rbx
    e071:	0f 84 55 01 00 00    	je     e1cc <code_add_into+0x3fc>
    e077:	48 8b 03             	mov    (%rbx),%rax
    e07a:	48 83 f8 10          	cmp    $0x10,%rax
    e07e:	0f 8e 48 01 00 00    	jle    e1cc <code_add_into+0x3fc>
    e084:	48 8d 3c c5 00 00 00 	lea    0x0(,%rax,8),%rdi
    e08b:	00 
    e08c:	e8 4f 41 ff ff       	call   21e0 <malloc@plt>
    e091:	48 89 04 24          	mov    %rax,(%rsp)
    e095:	48 85 c0             	test   %rax,%rax
    e098:	0f 84 db 05 00 00    	je     e679 <code_add_into+0x8a9>
    e09e:	48 c7 44 24 08 00 00 	movq   $0x0,0x8(%rsp)
    e0a5:	00 00 
    e0a7:	45 31 e4             	xor    %r12d,%r12d
    e0aa:	48 c7 44 24 10 00 00 	movq   $0x0,0x10(%rsp)
    e0b1:	00 00 
    e0b3:	48 85 db             	test   %rbx,%rbx
    e0b6:	0f 84 35 05 00 00    	je     e5f1 <code_add_into+0x821>
    e0bc:	4c 3b 23             	cmp    (%rbx),%r12
    e0bf:	0f 8d 2c 05 00 00    	jge    e5f1 <code_add_into+0x821>
    e0c5:	48 8b 53 08          	mov    0x8(%rbx),%rdx
    e0c9:	4a 8d 04 e5 00 00 00 	lea    0x0(,%r12,8),%rax
    e0d0:	00 
    e0d1:	48 89 44 24 18       	mov    %rax,0x18(%rsp)
    e0d6:	4a 8b 1c e2          	mov    (%rdx,%r12,8),%rbx
    e0da:	49 89 d5             	mov    %rdx,%r13
    e0dd:	48 8d 2c 02          	lea    (%rdx,%rax,1),%rbp
    e0e1:	4d 85 e4             	test   %r12,%r12
    e0e4:	74 27                	je     e10d <code_add_into+0x33d>
    e0e6:	66 2e 0f 1f 84 00 00 	cs nopw 0x0(%rax,%rax,1)
    e0ed:	00 00 00 
    e0f0:	49 8b 7d 00          	mov    0x0(%r13),%rdi
    e0f4:	48 89 de             	mov    %rbx,%rsi
    e0f7:	e8 64 40 ff ff       	call   2160 <strcmp@plt>
    e0fc:	85 c0                	test   %eax,%eax
    e0fe:	0f 84 ac 00 00 00    	je     e1b0 <code_add_into+0x3e0>
    e104:	49 83 c5 08          	add    $0x8,%r13
    e108:	4c 39 ed             	cmp    %r13,%rbp
    e10b:	75 e3                	jne    e0f0 <code_add_into+0x320>
    e10d:	8b 53 fc             	mov    -0x4(%rbx),%edx
    e110:	48 89 de             	mov    %rbx,%rsi
    e113:	4c 89 ff             	mov    %r15,%rdi
    e116:	e8 35 5d ff ff       	call   3e50 <field_position>
    e11b:	48 8b 0c 24          	mov    (%rsp),%rcx
    e11f:	4a 89 04 e1          	mov    %rax,(%rcx,%r12,8)
    e123:	41 8b 0e             	mov    (%r14),%ecx
    e126:	8d 51 fc             	lea    -0x4(%rcx),%edx
    e129:	48 85 c0             	test   %rax,%rax
    e12c:	0f 88 a8 00 00 00    	js     e1da <code_add_into+0x40a>
    e132:	83 fa 01             	cmp    $0x1,%edx
    e135:	0f 87 b6 04 00 00    	ja     e5f1 <code_add_into+0x821>
    e13b:	49 8b 5e 08          	mov    0x8(%r14),%rbx
    e13f:	49 83 c4 01          	add    $0x1,%r12
    e143:	e9 6b ff ff ff       	jmp    e0b3 <code_add_into+0x2e3>
    e148:	0f 1f 84 00 00 00 00 	nopl   0x0(%rax,%rax,1)
    e14f:	00 
    e150:	bf 60 00 00 00       	mov    $0x60,%edi
    e155:	ba 04 00 00 00       	mov    $0x4,%edx
    e15a:	bd 01 00 00 00       	mov    $0x1,%ebp
    e15f:	31 db                	xor    %ebx,%ebx
    e161:	41 bc 01 00 00 00    	mov    $0x1,%r12d
    e167:	e9 a3 fd ff ff       	jmp    df0f <code_add_into+0x13f>
    e16c:	0f 1f 40 00          	nopl   0x0(%rax)
    e170:	49 8b 56 08          	mov    0x8(%r14),%rdx
    e174:	48 85 d2             	test   %rdx,%rdx
    e177:	0f 84 b8 fe ff ff    	je     e035 <code_add_into+0x265>
    e17d:	48 8b 2a             	mov    (%rdx),%rbp
    e180:	48 85 ed             	test   %rbp,%rbp
    e183:	0f 84 ac fe ff ff    	je     e035 <code_add_into+0x265>
    e189:	49 8b 57 08          	mov    0x8(%r15),%rdx
    e18d:	48 85 d2             	test   %rdx,%rdx
    e190:	0f 84 dc 04 00 00    	je     e672 <code_add_into+0x8a2>
    e196:	48 8b 1a             	mov    (%rdx),%rbx
    e199:	4c 8d 64 1d 00       	lea    0x0(%rbp,%rbx,1),%r12
    e19e:	85 c0                	test   %eax,%eax
    e1a0:	0f 84 4f fd ff ff    	je     def5 <code_add_into+0x125>
    e1a6:	4d 8b 6f 08          	mov    0x8(%r15),%r13
    e1aa:	e9 c4 fd ff ff       	jmp    df73 <code_add_into+0x1a3>
    e1af:	90                   	nop
    e1b0:	48 8b 3c 24          	mov    (%rsp),%rdi
    e1b4:	48 8d 44 24 30       	lea    0x30(%rsp),%rax
    e1b9:	48 39 c7             	cmp    %rax,%rdi
    e1bc:	0f 84 48 fc ff ff    	je     de0a <code_add_into+0x3a>
    e1c2:	e8 89 3e ff ff       	call   2050 <free@plt>
    e1c7:	e9 3e fc ff ff       	jmp    de0a <code_add_into+0x3a>
    e1cc:	48 8d 44 24 30       	lea    0x30(%rsp),%rax
    e1d1:	48 89 04 24          	mov    %rax,(%rsp)
    e1d5:	e9 c4 fe ff ff       	jmp    e09e <code_add_into+0x2ce>
    e1da:	48 83 44 24 10 01    	addq   $0x1,0x10(%rsp)
    e1e0:	83 fa 01             	cmp    $0x1,%edx
    e1e3:	77 32                	ja     e217 <code_add_into+0x447>
    e1e5:	49 8b 5e 08          	mov    0x8(%r14),%rbx
    e1e9:	48 89 d8             	mov    %rbx,%rax
    e1ec:	48 85 db             	test   %rbx,%rbx
    e1ef:	74 04                	je     e1f5 <code_add_into+0x425>
    e1f1:	48 8b 43 08          	mov    0x8(%rbx),%rax
    e1f5:	48 8b 4c 24 18       	mov    0x18(%rsp),%rcx
    e1fa:	48 8b 3c 08          	mov    (%rax,%rcx,1),%rdi
    e1fe:	e8 dd 3e ff ff       	call   20e0 <strlen@plt>
    e203:	48 8b 4c 24 08       	mov    0x8(%rsp),%rcx
    e208:	48 8d 44 01 05       	lea    0x5(%rcx,%rax,1),%rax
    e20d:	48 89 44 24 08       	mov    %rax,0x8(%rsp)
    e212:	e9 28 ff ff ff       	jmp    e13f <code_add_into+0x36f>
    e217:	48 8b 44 24 18       	mov    0x18(%rsp),%rax
    e21c:	48 8b 38             	mov    (%rax),%rdi
    e21f:	e8 bc 3e ff ff       	call   20e0 <strlen@plt>
    e224:	48 8b 4c 24 08       	mov    0x8(%rsp),%rcx
    e229:	48 8d 44 01 05       	lea    0x5(%rcx,%rax,1),%rax
    e22e:	48 89 44 24 08       	mov    %rax,0x8(%rsp)
    e233:	4c 89 ff             	mov    %r15,%rdi
    e236:	e8 15 4b ff ff       	call   2d50 <drop_object_index>
    e23b:	4d 8b 67 08          	mov    0x8(%r15),%r12
    e23f:	bf 10 00 00 00       	mov    $0x10,%edi
    e244:	49 8b 74 24 08       	mov    0x8(%r12),%rsi
    e249:	49 8d 54 24 10       	lea    0x10(%r12),%rdx
    e24e:	48 89 f0             	mov    %rsi,%rax
    e251:	48 29 d0             	sub    %rdx,%rax
    e254:	48 99                	cqto
    e256:	48 f7 ff             	idiv   %rdi
    e259:	48 89 c5             	mov    %rax,%rbp
    e25c:	49 8b 04 24          	mov    (%r12),%rax
    e260:	48 8b 5c c6 f8       	mov    -0x8(%rsi,%rax,8),%rbx
    e265:	48 89 df             	mov    %rbx,%rdi
    e268:	e8 73 3e ff ff       	call   20e0 <strlen@plt>
    e26d:	4c 8d 6c 03 01       	lea    0x1(%rbx,%rax,1),%r13
    e272:	49 8d 44 24 f0       	lea    -0x10(%r12),%rax
    e277:	48 89 c7             	mov    %rax,%rdi
    e27a:	48 89 44 24 28       	mov    %rax,0x28(%rsp)
    e27f:	e8 9c 3e ff ff       	call   2120 <malloc_usable_size@plt>
    e284:	48 89 c2             	mov    %rax,%rdx
    e287:	48 8b 44 24 10       	mov    0x10(%rsp),%rax
    e28c:	49 03 04 24          	add    (%r12),%rax
    e290:	48 39 c5             	cmp    %rax,%rbp
    e293:	7c 19                	jl     e2ae <code_add_into+0x4de>
    e295:	4c 89 e6             	mov    %r12,%rsi
    e298:	48 8b 4c 24 08       	mov    0x8(%rsp),%rcx
    e29d:	4c 29 ee             	sub    %r13,%rsi
    e2a0:	48 8d 54 16 f0       	lea    -0x10(%rsi,%rdx,1),%rdx
    e2a5:	48 39 ca             	cmp    %rcx,%rdx
    e2a8:	0f 8d 0d 01 00 00    	jge    e3bb <code_add_into+0x5eb>
    e2ae:	48 83 f8 03          	cmp    $0x3,%rax
    e2b2:	48 8d 1c 00          	lea    (%rax,%rax,1),%rbx
    e2b6:	b8 04 00 00 00       	mov    $0x4,%eax
    e2bb:	4c 89 ea             	mov    %r13,%rdx
    e2be:	48 0f 4e d8          	cmovle %rax,%rbx
    e2c2:	49 8b 44 24 08       	mov    0x8(%r12),%rax
    e2c7:	48 8d 04 e8          	lea    (%rax,%rbp,8),%rax
    e2cb:	48 29 c2             	sub    %rax,%rdx
    e2ce:	48 8b 44 24 08       	mov    0x8(%rsp),%rax
    e2d3:	48 01 c2             	add    %rax,%rdx
    e2d6:	48 6b c3 18          	imul   $0x18,%rbx,%rax
    e2da:	48 8d 7c 50 30       	lea    0x30(%rax,%rdx,2),%rdi
    e2df:	e8 fc 3e ff ff       	call   21e0 <malloc@plt>
    e2e4:	48 89 c1             	mov    %rax,%rcx
    e2e7:	48 85 c0             	test   %rax,%rax
    e2ea:	0f 84 89 03 00 00    	je     e679 <code_add_into+0x8a9>
    e2f0:	66 0f 6f 05 48 2d 00 	movdqa 0x2d48(%rip),%xmm0        # 11040 <no_value+0x7c0>
    e2f7:	00 
    e2f8:	0f 11 00             	movups %xmm0,(%rax)
    e2fb:	f0 48 83 05 24 ee 00 	lock addq $0x1,0xee24(%rip)        # 1d128 <live_blocks>
    e302:	00 01 
    e304:	4c 8d 68 10          	lea    0x10(%rax),%r13
    e308:	48 89 d8             	mov    %rbx,%rax
    e30b:	48 c1 e0 04          	shl    $0x4,%rax
    e30f:	48 8d 44 01 20       	lea    0x20(%rcx,%rax,1),%rax
    e314:	48 89 44 24 20       	mov    %rax,0x20(%rsp)
    e319:	48 8d 1c d8          	lea    (%rax,%rbx,8),%rbx
    e31d:	49 8b 04 24          	mov    (%r12),%rax
    e321:	48 85 c0             	test   %rax,%rax
    e324:	7e 71                	jle    e397 <code_add_into+0x5c7>
    e326:	31 ed                	xor    %ebp,%ebp
    e328:	49 8b 44 24 08       	mov    0x8(%r12),%rax
    e32d:	48 89 4c 24 18       	mov    %rcx,0x18(%rsp)
    e332:	48 8b 34 e8          	mov    (%rax,%rbp,8),%rsi
    e336:	48 89 f7             	mov    %rsi,%rdi
    e339:	48 89 74 24 08       	mov    %rsi,0x8(%rsp)
    e33e:	e8 9d 3d ff ff       	call   20e0 <strlen@plt>
    e343:	48 8b 74 24 08       	mov    0x8(%rsp),%rsi
    e348:	48 89 df             	mov    %rbx,%rdi
    e34b:	48 8d 50 05          	lea    0x5(%rax),%rdx
    e34f:	48 83 ee 04          	sub    $0x4,%rsi
    e353:	48 89 54 24 08       	mov    %rdx,0x8(%rsp)
    e358:	e8 43 3e ff ff       	call   21a0 <memcpy@plt>
    e35d:	48 8b 4c 24 20       	mov    0x20(%rsp),%rcx
    e362:	48 8d 43 04          	lea    0x4(%rbx),%rax
    e366:	48 8b 54 24 08       	mov    0x8(%rsp),%rdx
    e36b:	48 89 04 e9          	mov    %rax,(%rcx,%rbp,8)
    e36f:	48 89 e8             	mov    %rbp,%rax
    e372:	48 8b 4c 24 18       	mov    0x18(%rsp),%rcx
    e377:	48 83 c5 01          	add    $0x1,%rbp
    e37b:	48 c1 e0 04          	shl    $0x4,%rax
    e37f:	48 01 d3             	add    %rdx,%rbx
    e382:	f3 41 0f 6f 44 04 10 	movdqu 0x10(%r12,%rax,1),%xmm0
    e389:	0f 11 44 01 20       	movups %xmm0,0x20(%rcx,%rax,1)
    e38e:	49 8b 04 24          	mov    (%r12),%rax
    e392:	48 39 c5             	cmp    %rax,%rbp
    e395:	7c 91                	jl     e328 <code_add_into+0x558>
    e397:	48 89 41 10          	mov    %rax,0x10(%rcx)
    e39b:	48 8b 44 24 20       	mov    0x20(%rsp),%rax
    e3a0:	48 8b 7c 24 28       	mov    0x28(%rsp),%rdi
    e3a5:	48 89 41 18          	mov    %rax,0x18(%rcx)
    e3a9:	e8 a2 3c ff ff       	call   2050 <free@plt>
    e3ae:	f0 48 83 2d 71 ed 00 	lock subq $0x1,0xed71(%rip)        # 1d128 <live_blocks>
    e3b5:	00 01 
    e3b7:	4d 89 6f 08          	mov    %r13,0x8(%r15)
    e3bb:	41 8b 07             	mov    (%r15),%eax
    e3be:	83 e8 04             	sub    $0x4,%eax
    e3c1:	83 f8 01             	cmp    $0x1,%eax
    e3c4:	0f 87 1d 02 00 00    	ja     e5e7 <code_add_into+0x817>
    e3ca:	49 8b 47 08          	mov    0x8(%r15),%rax
    e3ce:	45 31 ed             	xor    %r13d,%r13d
    e3d1:	48 85 c0             	test   %rax,%rax
    e3d4:	74 07                	je     e3dd <code_add_into+0x60d>
    e3d6:	4c 8b 28             	mov    (%rax),%r13
    e3d9:	48 8b 40 08          	mov    0x8(%rax),%rax
    e3dd:	41 8b 0e             	mov    (%r14),%ecx
    e3e0:	8d 51 fc             	lea    -0x4(%rcx),%edx
    e3e3:	83 fa 01             	cmp    $0x1,%edx
    e3e6:	0f 87 e3 01 00 00    	ja     e5cf <code_add_into+0x7ff>
    e3ec:	4a 8b 5c e8 f8       	mov    -0x8(%rax,%r13,8),%rbx
    e3f1:	48 89 df             	mov    %rbx,%rdi
    e3f4:	e8 e7 3c ff ff       	call   20e0 <strlen@plt>
    e3f9:	48 8d 44 03 01       	lea    0x1(%rbx,%rax,1),%rax
    e3fe:	48 89 44 24 08       	mov    %rax,0x8(%rsp)
    e403:	31 d2                	xor    %edx,%edx
    e405:	eb 6c                	jmp    e473 <code_add_into+0x6a3>
    e407:	66 0f 1f 84 00 00 00 	nopw   0x0(%rax,%rax,1)
    e40e:	00 00 
    e410:	41 8b 0f             	mov    (%r15),%ecx
    e413:	48 c1 e0 04          	shl    $0x4,%rax
    e417:	8d 79 fc             	lea    -0x4(%rcx),%edi
    e41a:	83 ff 01             	cmp    $0x1,%edi
    e41d:	0f 87 30 01 00 00    	ja     e553 <code_add_into+0x783>
    e423:	49 8b 7f 08          	mov    0x8(%r15),%rdi
    e427:	48 85 ff             	test   %rdi,%rdi
    e42a:	0f 84 23 01 00 00    	je     e553 <code_add_into+0x783>
    e430:	4c 8d 64 07 10       	lea    0x10(%rdi,%rax,1),%r12
    e435:	45 85 d2             	test   %r10d,%r10d
    e438:	74 16                	je     e450 <code_add_into+0x680>
    e43a:	8b 03                	mov    (%rbx),%eax
    e43c:	83 f8 01             	cmp    $0x1,%eax
    e43f:	0f 85 1d 01 00 00    	jne    e562 <code_add_into+0x792>
    e445:	48 8b 43 08          	mov    0x8(%rbx),%rax
    e449:	48 83 40 f0 01       	addq   $0x1,-0x10(%rax)
    e44e:	66 90                	xchg   %ax,%ax
    e450:	4c 89 e7             	mov    %r12,%rdi
    e453:	e8 48 64 ff ff       	call   48a0 <code_release>
    e458:	f3 0f 6f 03          	movdqu (%rbx),%xmm0
    e45c:	41 0f 11 04 24       	movups %xmm0,(%r12)
    e461:	41 8b 06             	mov    (%r14),%eax
    e464:	48 89 ea             	mov    %rbp,%rdx
    e467:	83 e8 04             	sub    $0x4,%eax
    e46a:	83 f8 01             	cmp    $0x1,%eax
    e46d:	0f 87 1d 01 00 00    	ja     e590 <code_add_into+0x7c0>
    e473:	49 8b 76 08          	mov    0x8(%r14),%rsi
    e477:	48 85 f6             	test   %rsi,%rsi
    e47a:	0f 84 10 01 00 00    	je     e590 <code_add_into+0x7c0>
    e480:	48 3b 16             	cmp    (%rsi),%rdx
    e483:	0f 8d 07 01 00 00    	jge    e590 <code_add_into+0x7c0>
    e489:	48 8d 6a 01          	lea    0x1(%rdx),%rbp
    e48d:	48 89 e8             	mov    %rbp,%rax
    e490:	48 c1 e0 04          	shl    $0x4,%rax
    e494:	48 8d 1c 06          	lea    (%rsi,%rax,1),%rbx
    e498:	48 8b 04 24          	mov    (%rsp),%rax
    e49c:	44 8b 53 04          	mov    0x4(%rbx),%r10d
    e4a0:	48 8b 04 d0          	mov    (%rax,%rdx,8),%rax
    e4a4:	48 85 c0             	test   %rax,%rax
    e4a7:	0f 89 63 ff ff ff    	jns    e410 <code_add_into+0x640>
    e4ad:	45 85 d2             	test   %r10d,%r10d
    e4b0:	74 14                	je     e4c6 <code_add_into+0x6f6>
    e4b2:	8b 03                	mov    (%rbx),%eax
    e4b4:	83 f8 01             	cmp    $0x1,%eax
    e4b7:	0f 85 c8 00 00 00    	jne    e585 <code_add_into+0x7b5>
    e4bd:	48 8b 43 08          	mov    0x8(%rbx),%rax
    e4c1:	48 83 40 f0 01       	addq   $0x1,-0x10(%rax)
    e4c6:	48 8b 46 08          	mov    0x8(%rsi),%rax
    e4ca:	48 8b 34 d0          	mov    (%rax,%rdx,8),%rsi
    e4ce:	49 8b 47 08          	mov    0x8(%r15),%rax
    e4d2:	48 8b 40 08          	mov    0x8(%rax),%rax
    e4d6:	48 89 f7             	mov    %rsi,%rdi
    e4d9:	48 89 74 24 18       	mov    %rsi,0x18(%rsp)
    e4de:	4e 8d 14 e8          	lea    (%rax,%r13,8),%r10
    e4e2:	4c 89 54 24 20       	mov    %r10,0x20(%rsp)
    e4e7:	e8 f4 3b ff ff       	call   20e0 <strlen@plt>
    e4ec:	48 8b 74 24 18       	mov    0x18(%rsp),%rsi
    e4f1:	48 8b 7c 24 08       	mov    0x8(%rsp),%rdi
    e4f6:	4c 8d 60 05          	lea    0x5(%rax),%r12
    e4fa:	4c 89 e2             	mov    %r12,%rdx
    e4fd:	48 83 ee 04          	sub    $0x4,%rsi
    e501:	e8 9a 3c ff ff       	call   21a0 <memcpy@plt>
    e506:	48 8b 4c 24 08       	mov    0x8(%rsp),%rcx
    e50b:	4c 8b 54 24 20       	mov    0x20(%rsp),%r10
    e510:	48 8d 41 04          	lea    0x4(%rcx),%rax
    e514:	4c 01 e1             	add    %r12,%rcx
    e517:	49 89 02             	mov    %rax,(%r10)
    e51a:	41 8b 07             	mov    (%r15),%eax
    e51d:	48 89 4c 24 08       	mov    %rcx,0x8(%rsp)
    e522:	8d 50 fc             	lea    -0x4(%rax),%edx
    e525:	31 c0                	xor    %eax,%eax
    e527:	83 fa 01             	cmp    $0x1,%edx
    e52a:	77 0f                	ja     e53b <code_add_into+0x76b>
    e52c:	49 8b 47 08          	mov    0x8(%r15),%rax
    e530:	48 8d 50 10          	lea    0x10(%rax),%rdx
    e534:	48 85 c0             	test   %rax,%rax
    e537:	48 0f 45 c2          	cmovne %rdx,%rax
    e53b:	4c 89 ea             	mov    %r13,%rdx
    e53e:	f3 0f 6f 03          	movdqu (%rbx),%xmm0
    e542:	49 83 c5 01          	add    $0x1,%r13
    e546:	48 c1 e2 04          	shl    $0x4,%rdx
    e54a:	0f 11 04 10          	movups %xmm0,(%rax,%rdx,1)
    e54e:	e9 0e ff ff ff       	jmp    e461 <code_add_into+0x691>
    e553:	49 89 c4             	mov    %rax,%r12
    e556:	45 85 d2             	test   %r10d,%r10d
    e559:	74 1c                	je     e577 <code_add_into+0x7a7>
    e55b:	8b 03                	mov    (%rbx),%eax
    e55d:	83 f8 01             	cmp    $0x1,%eax
    e560:	74 0c                	je     e56e <code_add_into+0x79e>
    e562:	83 e8 04             	sub    $0x4,%eax
    e565:	83 f8 01             	cmp    $0x1,%eax
    e568:	0f 87 bc 3f ff ff    	ja     252a <code_add_into.cold>
    e56e:	48 8b 43 08          	mov    0x8(%rbx),%rax
    e572:	48 83 40 f0 01       	addq   $0x1,-0x10(%rax)
    e577:	4d 85 e4             	test   %r12,%r12
    e57a:	0f 85 d0 fe ff ff    	jne    e450 <code_add_into+0x680>
    e580:	e9 41 ff ff ff       	jmp    e4c6 <code_add_into+0x6f6>
    e585:	45 31 e4             	xor    %r12d,%r12d
    e588:	eb d8                	jmp    e562 <code_add_into+0x792>
    e58a:	66 0f 1f 44 00 00    	nopw   0x0(%rax,%rax,1)
    e590:	48 83 7c 24 10 00    	cmpq   $0x0,0x10(%rsp)
    e596:	75 37                	jne    e5cf <code_add_into+0x7ff>
    e598:	49 8b 47 08          	mov    0x8(%r15),%rax
    e59c:	4c 89 6c 24 10       	mov    %r13,0x10(%rsp)
    e5a1:	48 89 44 24 08       	mov    %rax,0x8(%rsp)
    e5a6:	48 8b 44 24 08       	mov    0x8(%rsp),%rax
    e5ab:	48 8b 4c 24 10       	mov    0x10(%rsp),%rcx
    e5b0:	48 8b 3c 24          	mov    (%rsp),%rdi
    e5b4:	48 89 08             	mov    %rcx,(%rax)
    e5b7:	48 8d 44 24 30       	lea    0x30(%rsp),%rax
    e5bc:	48 39 c7             	cmp    %rax,%rdi
    e5bf:	0f 84 70 fa ff ff    	je     e035 <code_add_into+0x265>
    e5c5:	e8 86 3a ff ff       	call   2050 <free@plt>
    e5ca:	e9 66 fa ff ff       	jmp    e035 <code_add_into+0x265>
    e5cf:	49 8b 47 08          	mov    0x8(%r15),%rax
    e5d3:	4c 89 6c 24 10       	mov    %r13,0x10(%rsp)
    e5d8:	48 89 44 24 08       	mov    %rax,0x8(%rsp)
    e5dd:	48 c7 40 f8 00 00 00 	movq   $0x0,-0x8(%rax)
    e5e4:	00 
    e5e5:	eb bf                	jmp    e5a6 <code_add_into+0x7d6>
    e5e7:	45 31 ed             	xor    %r13d,%r13d
    e5ea:	31 c0                	xor    %eax,%eax
    e5ec:	e9 ec fd ff ff       	jmp    e3dd <code_add_into+0x60d>
    e5f1:	48 83 7c 24 10 00    	cmpq   $0x0,0x10(%rsp)
    e5f7:	0f 85 36 fc ff ff    	jne    e233 <code_add_into+0x463>
    e5fd:	41 8b 0f             	mov    (%r15),%ecx
    e600:	41 8b 06             	mov    (%r14),%eax
    e603:	8d 51 fc             	lea    -0x4(%rcx),%edx
    e606:	83 e8 04             	sub    $0x4,%eax
    e609:	83 fa 01             	cmp    $0x1,%edx
    e60c:	77 27                	ja     e635 <code_add_into+0x865>
    e60e:	49 8b 4f 08          	mov    0x8(%r15),%rcx
    e612:	48 89 4c 24 08       	mov    %rcx,0x8(%rsp)
    e617:	48 85 c9             	test   %rcx,%rcx
    e61a:	74 37                	je     e653 <code_add_into+0x883>
    e61c:	48 8b 4c 24 08       	mov    0x8(%rsp),%rcx
    e621:	4c 8b 29             	mov    (%rcx),%r13
    e624:	83 f8 01             	cmp    $0x1,%eax
    e627:	77 20                	ja     e649 <code_add_into+0x879>
    e629:	31 c0                	xor    %eax,%eax
    e62b:	48 89 44 24 08       	mov    %rax,0x8(%rsp)
    e630:	e9 ce fd ff ff       	jmp    e403 <code_add_into+0x633>
    e635:	83 f8 01             	cmp    $0x1,%eax
    e638:	77 2a                	ja     e664 <code_add_into+0x894>
    e63a:	31 d2                	xor    %edx,%edx
    e63c:	45 31 ed             	xor    %r13d,%r13d
    e63f:	48 89 54 24 08       	mov    %rdx,0x8(%rsp)
    e644:	e9 ba fd ff ff       	jmp    e403 <code_add_into+0x633>
    e649:	4c 89 6c 24 10       	mov    %r13,0x10(%rsp)
    e64e:	e9 53 ff ff ff       	jmp    e5a6 <code_add_into+0x7d6>
    e653:	45 31 ed             	xor    %r13d,%r13d
    e656:	83 f8 01             	cmp    $0x1,%eax
    e659:	0f 86 a4 fd ff ff    	jbe    e403 <code_add_into+0x633>
    e65f:	e9 42 ff ff ff       	jmp    e5a6 <code_add_into+0x7d6>
    e664:	49 8b 47 08          	mov    0x8(%r15),%rax
    e668:	48 89 44 24 08       	mov    %rax,0x8(%rsp)
    e66d:	e9 34 ff ff ff       	jmp    e5a6 <code_add_into+0x7d6>
    e672:	31 db                	xor    %ebx,%ebx
    e674:	e9 20 fb ff ff       	jmp    e199 <code_add_into+0x3c9>
    e679:	48 8d 3d 0e 1d 00 00 	lea    0x1d0e(%rip),%rdi        # 1038e <_fini+0x56e>
    e680:	e8 7b 53 ff ff       	call   3a00 <code_runtime_error>

Disassembly of section .fini:
